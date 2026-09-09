use anyhow::{Context, Result};
use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDay {
    pub local_date: NaiveDate,
    pub timezone: String,
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
}

impl ResolvedDay {
    pub fn duration(&self) -> Duration {
        self.end_utc - self.start_utc
    }
}

pub fn resolve_day(local_date: NaiveDate, timezone: &str) -> Result<ResolvedDay> {
    let timezone = timezone
        .parse::<Tz>()
        .with_context(|| format!("invalid IANA timezone: {timezone}"))?;
    let next_date = local_date
        .succ_opt()
        .context("daily date has no following calendar day")?;
    let start_utc = first_valid_instant(local_date, timezone)?;
    let end_utc = first_valid_instant(next_date, timezone)?;
    anyhow::ensure!(end_utc > start_utc, "resolved day has an invalid UTC range");
    Ok(ResolvedDay {
        local_date,
        timezone: timezone.name().to_owned(),
        start_utc,
        end_utc,
    })
}

fn first_valid_instant(date: NaiveDate, timezone: Tz) -> Result<DateTime<Utc>> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .context("invalid local midnight")?;
    for minutes in 0..=(24 * 60) {
        let candidate: NaiveDateTime = midnight + Duration::minutes(minutes);
        match timezone.from_local_datetime(&candidate) {
            LocalResult::Single(value) => return Ok(value.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                return Ok(first.min(second).with_timezone(&Utc));
            }
            LocalResult::None => {}
        }
    }
    anyhow::bail!("timezone has no valid instant for local date {date}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_normal_stockholm_day_on_the_server() {
        let day = resolve_day(
            NaiveDate::from_ymd_opt(2026, 9, 7).expect("date"),
            "Europe/Stockholm",
        )
        .expect("day resolves");
        assert_eq!(day.start_utc.to_rfc3339(), "2026-09-06T22:00:00+00:00");
        assert_eq!(day.end_utc.to_rfc3339(), "2026-09-07T22:00:00+00:00");
        assert_eq!(day.duration(), Duration::hours(24));
    }

    #[test]
    fn resolves_dst_days_without_assuming_twenty_four_hours() {
        let spring = resolve_day(
            NaiveDate::from_ymd_opt(2026, 3, 29).expect("date"),
            "Europe/Stockholm",
        )
        .expect("spring day resolves");
        let autumn = resolve_day(
            NaiveDate::from_ymd_opt(2026, 10, 25).expect("date"),
            "Europe/Stockholm",
        )
        .expect("autumn day resolves");
        assert_eq!(spring.duration(), Duration::hours(23));
        assert_eq!(autumn.duration(), Duration::hours(25));
    }

    #[test]
    fn rejects_unknown_timezones() {
        assert!(
            resolve_day(
                NaiveDate::from_ymd_opt(2026, 9, 7).expect("date"),
                "Browser/Local",
            )
            .is_err()
        );
    }
}
