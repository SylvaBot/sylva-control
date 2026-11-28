//! How an activity reads: the same groups, icons and sentences as Sylva's own activity lists.

use crate::format::{money, num, numf};
use pine_core::{Activity, ActivityKind};
use serde_json::Value;

pub struct Group {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

pub const GROUPS: &[Group] = &[
    Group { id: "follow", label: "Follows", icon: "heart" },
    Group { id: "sub", label: "Subs", icon: "star" },
    Group { id: "gift", label: "Gifts", icon: "gift" },
    Group { id: "cheer", label: "Bits", icon: "zap" },
    Group { id: "raid", label: "Raids", icon: "raid" },
    Group { id: "tip", label: "Tips", icon: "tip" },
    Group { id: "redemption", label: "Redemptions", icon: "gem" },
    Group { id: "other", label: "Other", icon: "sparkles" },
];

pub fn group_of(kind: ActivityKind) -> &'static Group {
    use ActivityKind::*;
    let id = match kind {
        Follow => "follow",
        Subscriber => "sub",
        GiftedSub | CommunityGift => "gift",
        Cheer => "cheer",
        Raid | Host => "raid",
        Tip => "tip",
        Redemption => "redemption",
        Other => "other",
    };
    GROUPS.iter().find(|g| g.id == id).unwrap()
}

/// The filter chips on the Activity page: All, then every group but Other.
pub fn chips() -> Vec<(&'static str, &'static str)> {
    std::iter::once(("all", "All")).chain(GROUPS.iter().filter(|g| g.id != "other").map(|g| (g.id, g.label))).collect()
}

/// One sentence, as runs of text that are bold (names) or not, plus small pills and a message.
pub struct Desc {
    pub parts: Vec<(String, bool)>,
    pub pills: Vec<String>,
    pub message: Option<String>,
}

fn tier_label(t: Option<&str>) -> Option<String> {
    Some(match t? {
        "1000" => "Tier 1",
        "2000" => "Tier 2",
        "3000" => "Tier 3",
        "prime" => "Prime",
        _ => return None,
    }
    .into())
}

pub fn describe(a: &Activity) -> Desc {
    let name = a.display_name.clone().or(a.username.clone()).unwrap_or_else(|| "Someone".into());
    let n = a.amount;
    let one = n == Some(1.0);
    let tier = tier_label(a.tier.as_deref());
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut pills = Vec::new();
    let mut message = a.message.clone().filter(|m| !m.trim().is_empty());
    let b = |t: &str, parts: &mut Vec<(String, bool)>| parts.push((t.to_string(), true));
    let t = |s: &str, parts: &mut Vec<(String, bool)>| parts.push((s.to_string(), false));
    let gift_note = |m: &Option<String>| m.as_ref().is_some_and(|m| m.to_lowercase().contains("gift"));
    match a.kind {
        ActivityKind::Follow => {
            b(&name, &mut parts);
            t(" followed", &mut parts);
        }
        ActivityKind::Subscriber => {
            b(&name, &mut parts);
            let re = n.is_some_and(|n| n > 1.0);
            t(if re { " resubscribed" } else { " subscribed" }, &mut parts);
            pills.extend(tier);
            if re {
                pills.push(format!("{} months", numf(n.unwrap())));
            }
        }
        ActivityKind::GiftedSub => {
            if let Some(s) = &a.sender {
                b(s, &mut parts);
                t(" gifted a sub to ", &mut parts);
                b(&name, &mut parts);
            } else {
                b(&name, &mut parts);
                t(" got a gifted sub", &mut parts);
            }
            pills.extend(tier);
            if gift_note(&message) {
                message = None;
            }
        }
        ActivityKind::CommunityGift => {
            b(&name, &mut parts);
            match n {
                Some(n) => t(&format!(" gifted {} {}", numf(n), if one { "sub" } else { "subs" }), &mut parts),
                None => t(" gifted subs", &mut parts),
            }
            pills.extend(tier);
            if gift_note(&message) {
                message = None;
            }
        }
        ActivityKind::Cheer => {
            b(&name, &mut parts);
            match n {
                Some(n) => t(&format!(" cheered {} {}", numf(n), if one { "bit" } else { "bits" }), &mut parts),
                None => t(" cheered", &mut parts),
            }
        }
        ActivityKind::Tip => {
            b(&name, &mut parts);
            match n {
                Some(n) => t(&format!(" tipped {}", money(n, a.currency.as_deref())), &mut parts),
                None => t(" tipped", &mut parts),
            }
        }
        ActivityKind::Redemption => {
            b(&name, &mut parts);
            t(" redeemed ", &mut parts);
            b(a.redemption.as_deref().unwrap_or("a reward"), &mut parts);
        }
        ActivityKind::Raid | ActivityKind::Host => {
            b(&name, &mut parts);
            let verb = if a.kind == ActivityKind::Raid { "raided" } else { "hosted" };
            match n {
                Some(n) => t(&format!(" {verb} with {} {}", numf(n), if one { "viewer" } else { "viewers" }), &mut parts),
                None => t(&format!(" {verb}"), &mut parts),
            }
        }
        ActivityKind::Other => return other_words(a),
    }
    Desc { parts, pills, message }
}

fn secs(n: u64) -> String {
    if n < 60 {
        return format!("{n} seconds");
    }
    let (m, r) = (n / 60, n % 60);
    if r > 0 {
        format!("{m}:{r:02}")
    } else {
        format!("{m} {}", if m == 1 { "minute" } else { "minutes" })
    }
}

/// Words for Twitch events that are not follows, subs and the like, from the payload Sylva
/// passes along.
fn other_words(a: &Activity) -> Desc {
    let null = Value::Null;
    let raw = a.raw.as_ref().unwrap_or(&null);
    let str_ = |k: &str| raw.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let int = |k: &str| raw.get(k).and_then(Value::as_u64);
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut pills = Vec::new();
    let mut message = a.message.clone().filter(|m| !m.trim().is_empty());
    let b = |t: &str, p: &mut Vec<(String, bool)>| p.push((t.to_string(), true));
    let t = |s: &str, p: &mut Vec<(String, bool)>| p.push((s.to_string(), false));
    let ty = a.raw_type.as_str();
    let ends = |x: &str| ty.ends_with(x);
    let winner = |list: Option<&Value>, id: Option<String>| -> Option<String> {
        let list = list?.as_array()?;
        if let Some(id) = id {
            return list.iter().find(|o| o["id"].as_str() == Some(id.as_str()))?["title"].as_str().map(str::to_string);
        }
        let score = |o: &Value| o["votes"].as_f64().or(o["channel_points"].as_f64()).unwrap_or(0.0);
        let best = list.iter().max_by(|x, y| score(x).total_cmp(&score(y)))?;
        (score(best) > 0.0).then(|| best["title"].as_str().map(str::to_string)).flatten()
    };
    match ty {
        "stream.online" => {
            b("You", &mut parts);
            t(" went live", &mut parts);
        }
        "stream.offline" => {
            b("Stream", &mut parts);
            t(" ended", &mut parts);
        }
        "channel.update" => {
            b("Stream info", &mut parts);
            t(" changed", &mut parts);
            let m: Vec<String> = [str_("title"), str_("category_name")].into_iter().flatten().collect();
            message = (!m.is_empty()).then(|| m.join(" · "));
        }
        "channel.raid" => {
            b("You", &mut parts);
            t(" raided ", &mut parts);
            b(&str_("to_broadcaster_user_name").unwrap_or_else(|| "a channel".into()), &mut parts);
            if let Some(n) = a.amount.filter(|n| *n > 0.0) {
                pills.push(format!("{} {}", numf(n), if n == 1.0 { "viewer" } else { "viewers" }));
            }
        }
        "channel.ad_break.begin" => {
            b("Ad break", &mut parts);
            t(" started", &mut parts);
            if let Some(d) = int("duration_seconds").filter(|d| *d > 0) {
                pills.push(secs(d));
            }
            if raw["is_automatic"].as_bool() == Some(true) {
                pills.push("Automatic".into());
            } else if let Some(r) = str_("requester_user_name") {
                pills.push(format!("Started by {r}"));
            }
        }
        "channel.hype_train.begin" | "channel.hype_train.progress" | "channel.hype_train.end" => {
            b("Hype train", &mut parts);
            t(if ends("begin") { " started" } else if ends("end") { " ended" } else { " is going" }, &mut parts);
            if let Some(l) = int("level").filter(|l| *l > 0) {
                pills.push(format!("Level {}", num(Some(l))));
            }
            if let Some(tot) = int("total").filter(|l| *l > 0) {
                pills.push(format!("{} points", num(Some(tot))));
            }
        }
        "channel.prediction.begin" | "channel.prediction.lock" | "channel.prediction.end" => {
            b("Prediction", &mut parts);
            t(if ends("begin") { " started" } else if ends("lock") { " locked" } else { " ended" }, &mut parts);
            message = str_("title");
            if ends("end") {
                pills.push(match winner(raw.get("outcomes"), str_("winning_outcome_id")) {
                    Some(w) => format!("Won: {w}"),
                    None => "Canceled".into(),
                });
            }
        }
        "channel.poll.begin" | "channel.poll.end" => {
            b("Poll", &mut parts);
            t(if ends("begin") { " started" } else { " ended" }, &mut parts);
            message = str_("title");
            if ends("end") {
                if let Some(top) = winner(raw.get("choices"), None) {
                    pills.push(format!("Top: {top}"));
                }
            }
        }
        "channel.charity_campaign.donate" => {
            b(&str_("user_name").or(a.display_name.clone()).unwrap_or_else(|| "Someone".into()), &mut parts);
            match str_("charity_name") {
                Some(c) => {
                    t(" donated to ", &mut parts);
                    b(&c, &mut parts);
                }
                None => t(" donated to charity", &mut parts),
            }
            let amt = &raw["amount"];
            if let Some(v) = amt["value"].as_f64() {
                let dp = amt["decimal_places"].as_i64().unwrap_or(0) as i32;
                pills.push(money(v / 10f64.powi(dp), amt["currency"].as_str().or(Some("USD"))));
            }
        }
        _ => {
            let words = ty.trim_start_matches("channel.").replace(['.', '_'], " ");
            let words = words.trim();
            let human = if words.is_empty() {
                "Event".to_string()
            } else {
                let mut c = words.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            };
            match a.display_name.clone().or(a.username.clone()) {
                Some(n) => {
                    b(&n, &mut parts);
                    t(&format!(" {}", human.to_lowercase()), &mut parts);
                }
                None => b(&human, &mut parts),
            }
        }
    }
    Desc { parts, pills, message }
}

/// Whether an activity passes the Activity page's chip and search box.
pub fn matches(a: &Activity, show: &str, query: &str) -> bool {
    if show != "all" && group_of(a.kind).id != show {
        return false;
    }
    if query.is_empty() {
        return true;
    }
    let hay = [&a.display_name, &a.username, &a.sender, &a.message, &a.redemption]
        .iter()
        .filter_map(|s| s.as_deref())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    hay.contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(kind: ActivityKind) -> Activity {
        Activity {
            id: "1".into(),
            kind,
            raw_type: String::new(),
            provider: None,
            created_at: None,
            username: Some("viewer".into()),
            display_name: Some("Viewer".into()),
            avatar: None,
            amount: None,
            currency: None,
            message: None,
            tier: None,
            gifted: false,
            sender: None,
            redemption: None,
            raw: None,
            test: false,
            received_at: None,
        }
    }

    fn sentence(d: &Desc) -> String {
        d.parts.iter().map(|p| p.0.as_str()).collect()
    }

    #[test]
    fn resubs_and_gifts_read_like_sylva() {
        let mut a = act(ActivityKind::Subscriber);
        a.amount = Some(5.0);
        a.tier = Some("1000".into());
        let d = describe(&a);
        assert_eq!(sentence(&d), "Viewer resubscribed");
        assert_eq!(d.pills, vec!["Tier 1".to_string(), "5 months".to_string()]);

        let mut g = act(ActivityKind::GiftedSub);
        g.sender = Some("Gifter".into());
        g.message = Some("Gifter gifted a sub to Viewer".into());
        let d = describe(&g);
        assert_eq!(sentence(&d), "Gifter gifted a sub to Viewer");
        assert!(d.message.is_none());
    }

    #[test]
    fn predictions_name_the_winner() {
        let mut a = act(ActivityKind::Other);
        a.raw_type = "channel.prediction.end".into();
        a.raw = Some(serde_json::json!({"title": "Win?", "winning_outcome_id": "b", "outcomes": [{"id": "a", "title": "Yes"}, {"id": "b", "title": "No"}]}));
        let d = describe(&a);
        assert_eq!(sentence(&d), "Prediction ended");
        assert_eq!(d.pills, vec!["Won: No".to_string()]);
        assert_eq!(d.message.as_deref(), Some("Win?"));
    }
}
