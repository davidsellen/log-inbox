use anyhow::{Context, Result};
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
};
use chrono_tz::Tz;
use std::path::{Component, Path};

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

pub fn resolve_local_time(
    local_date: NaiveDate,
    local_time: NaiveTime,
    timezone: &str,
) -> Result<DateTime<Utc>> {
    let timezone = timezone
        .parse::<Tz>()
        .with_context(|| format!("invalid IANA timezone: {timezone}"))?;
    let requested = local_date.and_time(local_time);
    for minutes in 0..=(24 * 60) {
        let candidate = requested + Duration::minutes(minutes);
        anyhow::ensure!(
            candidate.date() == local_date,
            "timezone has no valid scheduled instant on local date {local_date}"
        );
        match timezone.from_local_datetime(&candidate) {
            LocalResult::Single(value) => return Ok(value.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                return Ok(first.min(second).with_timezone(&Utc));
            }
            LocalResult::None => {}
        }
    }
    anyhow::bail!("timezone has no valid scheduled instant on local date {local_date}")
}

pub fn render_daily_path(root: &str, pattern: &str, local_date: NaiveDate) -> Result<String> {
    anyhow::ensure!(!pattern.trim().is_empty(), "daily pattern is required");
    let escaped_open = "\u{E000}";
    let escaped_close = "\u{E001}";
    let mut rendered = pattern
        .replace("{{", escaped_open)
        .replace("}}", escaped_close);
    let replacements = [
        ("{year}", local_date.format("%Y").to_string()),
        ("{month}", local_date.format("%m").to_string()),
        ("{month_name}", local_date.format("%b").to_string()),
        ("{day}", local_date.day().to_string()),
        ("{date}", local_date.format("%Y-%m-%d").to_string()),
    ];
    for (token, value) in replacements {
        rendered = rendered.replace(token, &value);
    }
    anyhow::ensure!(
        !rendered.contains('{') && !rendered.contains('}'),
        "daily pattern contains an unsupported token"
    );
    rendered = rendered
        .replace(escaped_open, "{")
        .replace(escaped_close, "}");
    let combined = [root.trim_matches('/'), rendered.trim_matches('/')]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    let path = Path::new(&combined);
    anyhow::ensure!(
        !combined.is_empty()
            && !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "rendered daily path must remain inside the workspace"
    );
    anyhow::ensure!(
        path.extension().and_then(|value| value.to_str()) == Some("md"),
        "rendered daily path must end in .md"
    );
    Ok(combined)
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

    #[test]
    fn resolves_ambiguous_and_nonexistent_scheduler_times_deterministically() {
        let spring = resolve_local_time(
            NaiveDate::from_ymd_opt(2026, 3, 29).unwrap(),
            NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
            "Europe/Stockholm",
        )
        .unwrap();
        assert_eq!(spring.to_rfc3339(), "2026-03-29T01:00:00+00:00");

        let autumn = resolve_local_time(
            NaiveDate::from_ymd_opt(2026, 10, 25).unwrap(),
            NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
            "Europe/Stockholm",
        )
        .unwrap();
        assert_eq!(autumn.to_rfc3339(), "2026-10-25T00:30:00+00:00");
    }

    #[test]
    fn renders_a_bounded_daily_markdown_path() {
        let date = NaiveDate::from_ymd_opt(2026, 9, 7).expect("date");
        assert_eq!(
            render_daily_path(
                "Work Log",
                "{year}/{month_name}/Daily log {month_name} {day}.md",
                date
            )
            .expect("path renders"),
            "Work Log/2026/Sep/Daily log Sep 7.md"
        );
        assert_eq!(
            render_daily_path("", "Literal {{date}} {date}.md", date).expect("escape renders"),
            "Literal {date} 2026-09-07.md"
        );
        assert!(render_daily_path("", "../{date}.md", date).is_err());
        assert!(render_daily_path("", "{quarter}.md", date).is_err());
    }
}
