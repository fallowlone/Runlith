//! Minimal testable clock boundary for event timestamps.
//!
//! The accepted contract requires timestamp metadata but leaves clock
//! source and precision open. This layer therefore isolates acquisition
//! (milliseconds since the Unix epoch) from rendering (RFC 3339 UTC), so
//! event tests run deterministically on a fixed clock while production
//! uses the wall clock. Monotonic clocks are never fabricated into
//! wall-clock timestamps: only real epoch millis render into `occurred_at`.

/// Wall/fixed millisecond clock.
pub trait Clock {
    /// Milliseconds since the Unix epoch. Pre-epoch times saturate to 0.
    fn now_millis(&self) -> u64;
}

/// Production clock: real wall-clock time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}

/// Deterministic clock for tests.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_millis(&self) -> u64 {
        self.0
    }
}

/// Renders epoch millis as RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`, second
/// precision). Pure integer civil-date math (Howard Hinnant's algorithm),
/// std-only — no date library needed for second precision.
#[must_use]
pub fn format_rfc3339(millis: u64) -> String {
    let total_secs = millis / 1000;
    let days = (total_secs / 86_400) as i64;
    let time = total_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60,
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_renders_as_zeros() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_dates_render_exactly() {
        // 2024-01-01T00:00:00Z == 1704067200 Unix seconds.
        assert_eq!(format_rfc3339(1_704_067_200_000), "2024-01-01T00:00:00Z");
        // Day rollover and leap day.
        assert_eq!(format_rfc3339(86_400_000), "1970-01-02T00:00:00Z");
        assert_eq!(format_rfc3339(1_709_164_800_000), "2024-02-29T00:00:00Z");
        // Sub-second millis truncate to the second.
        assert_eq!(format_rfc3339(1_704_067_200_999), "2024-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_704_067_201_000), "2024-01-01T00:00:01Z");
    }

    #[test]
    fn fixed_clock_is_deterministic() {
        assert_eq!(FixedClock(7).now_millis(), 7);
        assert_eq!(
            format_rfc3339(FixedClock(0).now_millis()),
            "1970-01-01T00:00:00Z"
        );
    }
}
