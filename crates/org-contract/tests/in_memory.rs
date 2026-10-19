use org::model::{
    AccountId, AccountRecord, EnvelopeId, MembershipId, MembershipRecord, OrgError, OrgPolicy,
    OrganizationId, OrganizationRecord, SyncEnvelope, TeamId, TeamRecord, UsageEntry,
};
use org::port::OrgStorePort;
use org_contract::run_org_store_contract;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Inner {
    accounts: BTreeMap<AccountId, AccountRecord>,
    organizations: BTreeMap<OrganizationId, OrganizationRecord>,
    teams: BTreeMap<TeamId, TeamRecord>,
    memberships: BTreeMap<MembershipId, MembershipRecord>,
    policies: BTreeMap<(OrganizationId, u64), OrgPolicy>,
    envelopes: BTreeMap<EnvelopeId, SyncEnvelope>,
    heads: BTreeMap<OrganizationId, u64>,
    usage: Vec<UsageEntry>,
}

#[derive(Default, Clone)]
struct InMemory {
    inner: Arc<Mutex<Inner>>,
}

fn put_new<K: Ord + Clone, T: Clone + PartialEq>(
    map: &mut BTreeMap<K, T>,
    key: K,
    value: &T,
    id: &str,
) -> Result<(), OrgError> {
    match map.get(&key) {
        Some(existing) if existing == value => Ok(()),
        Some(_) => Err(OrgError::Conflict(id.to_owned())),
        None => {
            map.insert(key, value.clone());
            Ok(())
        }
    }
}

impl OrgStorePort for InMemory {
    fn put_account(&mut self, a: &AccountRecord) -> Result<(), OrgError> {
        a.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        put_new(&mut g.accounts, a.id.clone(), a, a.id.as_str())
    }
    fn load_account(&self, id: &AccountId) -> Result<Option<AccountRecord>, OrgError> {
        Ok(self.inner.lock().unwrap().accounts.get(id).cloned())
    }
    fn put_organization(&mut self, o: &OrganizationRecord) -> Result<(), OrgError> {
        o.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        put_new(&mut g.organizations, o.id.clone(), o, o.id.as_str())
    }
    fn load_organization(
        &self,
        id: &OrganizationId,
    ) -> Result<Option<OrganizationRecord>, OrgError> {
        Ok(self.inner.lock().unwrap().organizations.get(id).cloned())
    }
    fn update_organization(&mut self, o: &OrganizationRecord) -> Result<(), OrgError> {
        o.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        if !g.organizations.contains_key(&o.id) {
            return Err(OrgError::NotFound(o.id.to_string()));
        }
        g.organizations.insert(o.id.clone(), o.clone());
        Ok(())
    }
    fn put_team(&mut self, t: &TeamRecord) -> Result<(), OrgError> {
        t.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        put_new(&mut g.teams, t.id.clone(), t, t.id.as_str())
    }
    fn load_team(&self, id: &TeamId) -> Result<Option<TeamRecord>, OrgError> {
        Ok(self.inner.lock().unwrap().teams.get(id).cloned())
    }
    fn list_teams(&self, org_id: &OrganizationId) -> Result<Vec<TeamRecord>, OrgError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .teams
            .values()
            .filter(|t| &t.org_id == org_id)
            .cloned()
            .collect())
    }
    fn put_membership(&mut self, m: &MembershipRecord) -> Result<(), OrgError> {
        m.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        put_new(&mut g.memberships, m.id.clone(), m, m.id.as_str())
    }
    fn load_membership(&self, id: &MembershipId) -> Result<Option<MembershipRecord>, OrgError> {
        Ok(self.inner.lock().unwrap().memberships.get(id).cloned())
    }
    fn update_membership(&mut self, m: &MembershipRecord) -> Result<(), OrgError> {
        m.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        if !g.memberships.contains_key(&m.id) {
            return Err(OrgError::NotFound(m.id.to_string()));
        }
        g.memberships.insert(m.id.clone(), m.clone());
        Ok(())
    }
    fn list_memberships(&self, org_id: &OrganizationId) -> Result<Vec<MembershipRecord>, OrgError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .memberships
            .values()
            .filter(|m| &m.org_id == org_id)
            .cloned()
            .collect())
    }
    fn put_policy(&mut self, p: &OrgPolicy) -> Result<(), OrgError> {
        p.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        let key = (p.org_id.clone(), p.version);
        match g.policies.get(&key) {
            Some(existing) if existing == p => Ok(()),
            Some(_) => Err(OrgError::Conflict(p.id.to_string())),
            None => {
                g.policies.insert(key, p.clone());
                Ok(())
            }
        }
    }
    fn current_policy(&self, org_id: &OrganizationId) -> Result<Option<OrgPolicy>, OrgError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .policies
            .values()
            .filter(|p| &p.org_id == org_id)
            .max_by_key(|p| p.version)
            .cloned())
    }
    fn append_envelope(&mut self, e: &SyncEnvelope) -> Result<(), OrgError> {
        e.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        if g.envelopes.contains_key(&e.id) {
            return Err(OrgError::DuplicateEnvelope(e.id.to_string()));
        }
        g.envelopes.insert(e.id.clone(), e.clone());
        g.heads.insert(e.org_id.clone(), e.revision);
        Ok(())
    }
    fn load_envelope(&self, id: &EnvelopeId) -> Result<Option<SyncEnvelope>, OrgError> {
        Ok(self.inner.lock().unwrap().envelopes.get(id).cloned())
    }
    fn sync_head(&self, org_id: &OrganizationId) -> Result<u64, OrgError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .heads
            .get(org_id)
            .copied()
            .unwrap_or(0))
    }
    fn append_usage(&mut self, e: &UsageEntry) -> Result<(), OrgError> {
        e.revalidate()?;
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g
            .usage
            .iter()
            .find(|u| u.org_id == e.org_id && u.idempotency_key == e.idempotency_key)
        {
            if existing == e {
                return Ok(());
            }
            return Err(OrgError::Conflict(e.idempotency_key.clone()));
        }
        if g.usage.iter().any(|u| u.id == e.id) {
            return Err(OrgError::Conflict(e.id.to_string()));
        }
        g.usage.push(e.clone());
        Ok(())
    }
    fn list_usage(&self, org_id: &OrganizationId) -> Result<Vec<UsageEntry>, OrgError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .usage
            .iter()
            .filter(|u| &u.org_id == org_id)
            .cloned()
            .collect())
    }
}

#[test]
fn in_memory_store_satisfies_org_contract() {
    let mut store = InMemory::default();
    let reopened = store.clone();
    run_org_store_contract(&mut store, move || reopened.clone());
}
