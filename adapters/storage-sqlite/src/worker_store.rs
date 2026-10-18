//! SQLite worker seam (TASK-0200, ADR-0046). Lifecycle decisions
//! stay in the pure `worker` domain; decode revalidates every row.
use rusqlite::{params, Connection, OptionalExtension};
use worker::model::{
    CapabilityScope, Challenge, ChallengeId, DeviceId, DeviceRecord, DeviceState, JobId, JobStatus,
    WorkerError, WorkerJob,
};
use worker::port::WorkerStorePort;

use crate::task_store::SqliteStore;

fn backend(error: rusqlite::Error) -> WorkerError {
    WorkerError::BackendFailure {
        detail: error.to_string(),
    }
}

fn stored<T>(parsed: Result<T, WorkerError>, what: &str) -> Result<T, WorkerError> {
    parsed.map_err(|err| WorkerError::BackendFailure {
        detail: format!("stored {what} is malformed: {err}"),
    })
}

fn u64_to_i64(v: u64, f: &str) -> Result<i64, WorkerError> {
    i64::try_from(v).map_err(|_| WorkerError::InvalidInput(format!("{f} overflows i64")))
}

fn i64_to_u64(v: i64, f: &str) -> Result<u64, WorkerError> {
    u64::try_from(v).map_err(|_| WorkerError::BackendFailure {
        detail: format!("stored {f} is negative"),
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode<const N: usize>(raw: &str, what: &str) -> Result<[u8; N], WorkerError> {
    if raw.len() != N * 2 {
        return Err(WorkerError::BackendFailure {
            detail: format!("stored {what} has wrong length"),
        });
    }
    let bytes: Vec<u8> = (0..N)
        .map(|i| {
            u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).map_err(|_| {
                WorkerError::BackendFailure {
                    detail: format!("stored {what} is not hex"),
                }
            })
        })
        .collect::<Result<Vec<u8>, WorkerError>>()?;
    bytes.try_into().map_err(|_| WorkerError::BackendFailure {
        detail: format!("stored {what} has wrong length"),
    })
}

type DeviceRow = (String, String, String, String, String, String, i64);
type JobRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
);
type ChallengeRow = (String, String, String, String, i64, i64);

fn device_from(t: DeviceRow) -> Result<DeviceRecord, WorkerError> {
    let d = DeviceRecord {
        id: stored(DeviceId::new(t.0), "device id")?,
        name: t.1,
        public_key: hex_decode::<32>(&t.2, "public key")?,
        fingerprint: t.3,
        state: stored(DeviceState::parse(&t.4), "device state")?,
        enrolled_by: t.5,
        enrolled_at: i64_to_u64(t.6, "enrolled_at")?,
    };
    d.revalidate()?;
    Ok(d)
}

fn job_from(t: JobRow) -> Result<WorkerJob, WorkerError> {
    let j = WorkerJob {
        id: stored(JobId::new(t.0), "job id")?,
        objective: t.1,
        capability_scope: stored(CapabilityScope::parse(&t.2), "capability scope")?,
        authorized_by: t.3,
        status: stored(JobStatus::parse(&t.4), "job status")?,
        claimed_by: t
            .5
            .map(|raw| stored(DeviceId::new(raw), "claimed_by"))
            .transpose()?,
        created_at: i64_to_u64(t.6, "created_at")?,
        finished_at: t.7.map(|v| i64_to_u64(v, "finished_at")).transpose()?,
        evidence_ref: t.8,
    };
    j.revalidate()?;
    Ok(j)
}

fn challenge_from(t: ChallengeRow) -> Result<Challenge, WorkerError> {
    Ok(Challenge {
        id: stored(ChallengeId::new(t.0), "challenge id")?,
        device_id: stored(DeviceId::new(t.1), "device id")?,
        job_id: stored(JobId::new(t.2), "job id")?,
        nonce: hex_decode::<32>(&t.3, "nonce")?,
        used: t.4 != 0,
        created_at: i64_to_u64(t.5, "created_at")?,
    })
}

const DEVICE_COLUMNS: &str =
    "device_id, name, public_key, fingerprint, state, enrolled_by, enrolled_at";
const JOB_COLUMNS: &str = "job_id, objective, capability_scope, authorized_by, status, \
    claimed_by, created_at, finished_at, evidence_ref";
const CHALLENGE_COLUMNS: &str = "challenge_id, device_id, job_id, nonce, used, created_at";

fn load_device(conn: &Connection, id: &DeviceId) -> Result<Option<DeviceRecord>, WorkerError> {
    let row = conn
        .query_row(
            &format!("SELECT {DEVICE_COLUMNS} FROM worker_devices WHERE device_id = ?1"),
            params![id.as_str()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    row.map(device_from).transpose()
}

impl WorkerStorePort for SqliteStore {
    fn enroll_device(&mut self, d: &DeviceRecord) -> Result<(), WorkerError> {
        d.revalidate()?;
        if let Some(existing) = load_device(self.connection(), &d.id)? {
            if existing == *d {
                return Ok(());
            }
            return Err(WorkerError::Conflict(d.id.to_string()));
        }
        self.connection()
            .execute(
                &format!(
                    "INSERT INTO worker_devices ({DEVICE_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7)"
                ),
                params![
                    d.id.as_str(),
                    d.name,
                    hex_encode(&d.public_key),
                    d.fingerprint,
                    d.state.as_str(),
                    d.enrolled_by,
                    u64_to_i64(d.enrolled_at, "enrolled_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_device(&self, id: &DeviceId) -> Result<Option<DeviceRecord>, WorkerError> {
        load_device(self.connection(), id)
    }

    fn update_device(&mut self, d: &DeviceRecord) -> Result<(), WorkerError> {
        d.revalidate()?;
        let n = self
            .connection()
            .execute(
                "UPDATE worker_devices SET name=?2, public_key=?3, fingerprint=?4, state=?5, enrolled_by=?6, enrolled_at=?7 WHERE device_id=?1",
                params![
                    d.id.as_str(),
                    d.name,
                    hex_encode(&d.public_key),
                    d.fingerprint,
                    d.state.as_str(),
                    d.enrolled_by,
                    u64_to_i64(d.enrolled_at, "enrolled_at")?,
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(WorkerError::NotFound(d.id.to_string()));
        }
        Ok(())
    }

    fn list_devices(&self) -> Result<Vec<DeviceRecord>, WorkerError> {
        let mut stmt = self
            .connection()
            .prepare(&format!(
                "SELECT {DEVICE_COLUMNS} FROM worker_devices ORDER BY device_id"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(device_from(r.map_err(backend)?)?);
        }
        Ok(out)
    }

    fn offer_job(&mut self, j: &WorkerJob) -> Result<(), WorkerError> {
        j.revalidate()?;
        if let Some(existing) = self.load_job(&j.id)? {
            if existing == *j {
                return Ok(());
            }
            return Err(WorkerError::Conflict(j.id.to_string()));
        }
        self.connection()
            .execute(
                &format!(
                    "INSERT INTO worker_jobs ({JOB_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
                ),
                params![
                    j.id.as_str(),
                    j.objective,
                    j.capability_scope.as_str(),
                    j.authorized_by,
                    j.status.as_str(),
                    j.claimed_by.as_ref().map(|d| d.as_str()),
                    u64_to_i64(j.created_at, "created_at")?,
                    j.finished_at
                        .map(|v| u64_to_i64(v, "finished_at"))
                        .transpose()?,
                    j.evidence_ref,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_job(&self, id: &JobId) -> Result<Option<WorkerJob>, WorkerError> {
        let row = self
            .connection()
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM worker_jobs WHERE job_id = ?1"),
                params![id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(job_from).transpose()
    }

    fn update_job(&mut self, j: &WorkerJob) -> Result<(), WorkerError> {
        j.revalidate()?;
        let n = self
            .connection()
            .execute(
                "UPDATE worker_jobs SET objective=?2, capability_scope=?3, authorized_by=?4, status=?5, claimed_by=?6, created_at=?7, finished_at=?8, evidence_ref=?9 WHERE job_id=?1",
                params![
                    j.id.as_str(),
                    j.objective,
                    j.capability_scope.as_str(),
                    j.authorized_by,
                    j.status.as_str(),
                    j.claimed_by.as_ref().map(|d| d.as_str()),
                    u64_to_i64(j.created_at, "created_at")?,
                    j.finished_at
                        .map(|v| u64_to_i64(v, "finished_at"))
                        .transpose()?,
                    j.evidence_ref,
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(WorkerError::NotFound(j.id.to_string()));
        }
        Ok(())
    }

    fn list_jobs(&self) -> Result<Vec<WorkerJob>, WorkerError> {
        let mut stmt = self
            .connection()
            .prepare(&format!(
                "SELECT {JOB_COLUMNS} FROM worker_jobs ORDER BY job_id"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(job_from(r.map_err(backend)?)?);
        }
        Ok(out)
    }

    fn put_challenge(&mut self, c: &Challenge) -> Result<(), WorkerError> {
        self.connection()
            .execute(
                &format!("INSERT INTO worker_challenges ({CHALLENGE_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6)"),
                params![
                    c.id.as_str(),
                    c.device_id.as_str(),
                    c.job_id.as_str(),
                    hex_encode(&c.nonce),
                    i64::from(c.used),
                    u64_to_i64(c.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_challenge(&self, id: &ChallengeId) -> Result<Option<Challenge>, WorkerError> {
        let row = self
            .connection()
            .query_row(
                &format!(
                    "SELECT {CHALLENGE_COLUMNS} FROM worker_challenges WHERE challenge_id = ?1"
                ),
                params![id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(challenge_from).transpose()
    }

    fn update_challenge(&mut self, c: &Challenge) -> Result<(), WorkerError> {
        let n = self
            .connection()
            .execute(
                "UPDATE worker_challenges SET device_id=?2, job_id=?3, nonce=?4, used=?5, created_at=?6 WHERE challenge_id=?1",
                params![
                    c.id.as_str(),
                    c.device_id.as_str(),
                    c.job_id.as_str(),
                    hex_encode(&c.nonce),
                    i64::from(c.used),
                    u64_to_i64(c.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(WorkerError::NotFound(c.id.to_string()));
        }
        Ok(())
    }
}
