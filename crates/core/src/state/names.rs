//! Names the engine makes from state: a job's id (1.4.2).

use mork_types::{AgentKey, JobId};

/// `<yyyymmdd-hhmmss>-<owner>-<4 hex>`, the owner's `/` made `-`.
pub fn job_id(now_ms: i64, owner: &AgentKey, salt: u16) -> JobId {
    let (y, mo, d, h, mi, s) = civil_from_ms(now_ms);
    let owner = owner.as_str().replace('/', "-");
    JobId::new(format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}-{owner}-{salt:04x}"))
}

/// UTC civil time from epoch milliseconds (Howard Hinnant's days-from-civil, inverted).
pub fn civil_from_ms(ms: i64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let as_u32 = |v: i64| u32::try_from(v).unwrap_or(0);
    (y, as_u32(m), as_u32(d), as_u32(rem / 3600), as_u32(rem % 3600 / 60), as_u32(rem % 60))
}
