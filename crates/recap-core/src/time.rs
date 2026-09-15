use chrono::{Datelike, DateTime, NaiveDate, NaiveTime, TimeZone, Utc};

use crate::models::Period;

/// Parse a period string ("day" | "week" | "month") and an optional ISO date string
/// into a `Period` and the corresponding UTC start/end timestamps.
pub fn parse_period_range(
    period: &str,
    date: Option<&str>,
) -> Result<(Period, DateTime<Utc>, DateTime<Utc>), String> {
    let base_date = match date {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|e| format!("invalid date: {e}"))?,
        None => Utc::now().date_naive(),
    };

    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();

    match period {
        "day" => {
            let start = Utc.from_utc_datetime(&base_date.and_time(midnight));
            let end = start + chrono::Duration::days(1);
            Ok((Period::Day(base_date), start, end))
        }
        "week" => {
            let weekday = base_date.weekday().num_days_from_monday();
            let week_start = base_date - chrono::Duration::days(weekday as i64);
            let start = Utc.from_utc_datetime(&week_start.and_time(midnight));
            let end = start + chrono::Duration::weeks(1);
            Ok((Period::Week(week_start), start, end))
        }
        "month" => {
            let month_start = NaiveDate::from_ymd_opt(base_date.year(), base_date.month(), 1)
                .ok_or("invalid month start")?;
            let next_month = if base_date.month() == 12 {
                NaiveDate::from_ymd_opt(base_date.year() + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(base_date.year(), base_date.month() + 1, 1)
            }
            .ok_or("invalid next month")?;

            let start = Utc.from_utc_datetime(&month_start.and_time(midnight));
            let end = Utc.from_utc_datetime(&next_month.and_time(midnight));
            Ok((Period::Month(month_start), start, end))
        }
        other => Err(format!("unknown period: {other} (expected day, week, or month)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_range_is_one_utc_day() {
        let (period, start, end) = parse_period_range("day", Some("2026-09-15")).unwrap();
        assert_eq!(period, Period::Day(NaiveDate::from_ymd_opt(2026, 9, 15).unwrap()));
        assert_eq!(start.to_rfc3339(), "2026-09-15T00:00:00+00:00");
        assert_eq!(end - start, chrono::Duration::days(1));
    }

    #[test]
    fn week_starts_on_monday() {
        // 2026-09-17 is a Thursday; the week should start Monday 2026-09-14.
        let (period, start, end) = parse_period_range("week", Some("2026-09-17")).unwrap();
        assert_eq!(period, Period::Week(NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()));
        assert_eq!(start.to_rfc3339(), "2026-09-14T00:00:00+00:00");
        assert_eq!(end - start, chrono::Duration::weeks(1));
    }

    #[test]
    fn month_wraps_year_in_december() {
        let (period, start, end) = parse_period_range("month", Some("2025-12-15")).unwrap();
        assert_eq!(period, Period::Month(NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()));
        assert_eq!(start.to_rfc3339(), "2025-12-01T00:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-01-01T00:00:00+00:00");
    }

    #[test]
    fn rejects_unknown_period_and_bad_date() {
        assert!(parse_period_range("year", None).is_err());
        assert!(parse_period_range("day", Some("15/09/2026")).is_err());
    }
}
