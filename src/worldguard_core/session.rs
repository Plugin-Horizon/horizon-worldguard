//! `weather-lock` and `time-lock` (WeatherLockFlag, TimeLockFlag): what a player sees while a region locks it, and the
//! bytes of the packets that show it. The engine has no per-player weather or time call, so the mod sends the two
//! vanilla packets itself (`game_event` for rain and thunder, `set_time` for the clock) to that player alone.

use std::collections::HashMap;

/// `TimeLockFlag.timePattern` (`([+\-])?\d+`) and `updatePlayerTime`: the time and whether it is relative (a leading
/// sign). Anything else is ignored, as invalid input is.
pub fn parse_time(value: &str) -> Option<(i64, bool)> {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let relative = value.starts_with(['+', '-']);
    value.strip_prefix('+').unwrap_or(value).parse::<i64>().ok().map(|t| (t, relative))
}

/// The locked weather as (raining, thundering). WorldGuard's `clear`, `rain` and `thunder`; Bukkit has only clear and
/// downfall for a player, so `rain` is rain without thunder and `thunder` is rain with it.
pub fn weather_levels(value: &str) -> Option<(bool, bool)> {
    match value.strip_prefix("minecraft:").unwrap_or(value) {
        "clear" => Some((false, false)),
        "rain" => Some((true, false)),
        "thunder" => Some((true, true)),
        _ => None,
    }
}

/// `ClientboundGameEventPacket`: the event id and a float.
pub fn game_event_body(event: u8, value: f32) -> Vec<u8> {
    let mut b = vec![event];
    b.extend_from_slice(&value.to_be_bytes());
    b
}

/// The game events that show a weather: start or stop of rain (1, 2), then the rain level (7) and the thunder level (8).
pub fn weather_events(raining: bool, thundering: bool) -> Vec<Vec<u8>> {
    weather_level_events(if raining { 1.0 } else { 0.0 }, if thundering { 1.0 } else { 0.0 })
}

/// The same for exact levels (the world's own weather, as it is now).
pub fn weather_level_events(rain: f32, thunder: f32) -> Vec<Vec<u8>> {
    vec![
        game_event_body(if rain > 0.2 { 1 } else { 2 }, 0.0),
        game_event_body(7, rain),
        game_event_body(8, thunder),
    ]
}

fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// `ClientboundSetTimePacket`: the game time, then one state per world clock (its registry id, total ticks, partial
/// tick and rate). A rate of 0 stops the clock on the client, which is how an absolute time is held.
pub fn set_time_body(game_time: i64, clock: i32, total_ticks: i64, rate: f32) -> Vec<u8> {
    let mut b = game_time.to_be_bytes().to_vec();
    varint(1, &mut b);
    varint(clock as u32 as u64, &mut b);
    varint(total_ticks as u64, &mut b);
    b.extend_from_slice(&0f32.to_be_bytes());
    b.extend_from_slice(&rate.to_be_bytes());
    b
}

/// What to do for one player this tick.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Show this weather (raining, thundering).
    Weather(bool, bool),
    /// Back to the world's own weather.
    ResetWeather,
    /// Show this time (ticks, relative to the world's clock when true); sent every tick, since the server's own
    /// time sync would otherwise win.
    Time(i64, bool),
    /// Back to the world's own time.
    ResetTime,
}

/// Which locks a player is under, to tell the change and the way out (`FlagValueChangeHandler`).
#[derive(Default)]
pub struct Sessions {
    last: HashMap<u128, (Option<(bool, bool)>, bool)>,
}

impl Sessions {
    /// `weather` and `time` are the flag values at the player's feet (None: no region sets one).
    pub fn step(&mut self, uuid: u128, weather: Option<&str>, time: Option<&str>) -> Vec<Action> {
        let weather = weather.and_then(weather_levels);
        let time = time.and_then(parse_time);
        let (was_weather, was_time) = self.last.get(&uuid).copied().unwrap_or((None, false));
        let mut out = Vec::new();
        if weather != was_weather {
            out.push(match weather {
                Some((r, t)) => Action::Weather(r, t),
                None => Action::ResetWeather,
            });
        }
        match time {
            Some((t, rel)) => out.push(Action::Time(t, rel)),
            None if was_time => out.push(Action::ResetTime),
            None => {}
        }
        if weather.is_none() && time.is_none() {
            self.last.remove(&uuid);
        } else {
            self.last.insert(uuid, (weather, time.is_some()));
        }
        out
    }

    /// A player who left: nothing to restore on a client that is gone.
    pub fn forget(&mut self, uuid: u128) {
        self.last.remove(&uuid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_values_follow_the_time_pattern() {
        assert_eq!(parse_time("6000"), Some((6000, false)));
        assert_eq!(parse_time("+100"), Some((100, true)));
        assert_eq!(parse_time("-100"), Some((-100, true)));
        for bad in ["", "+", "12a", "1.5", " 5", "noon"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn weather_names() {
        assert_eq!(weather_levels("minecraft:clear"), Some((false, false)));
        assert_eq!(weather_levels("rain"), Some((true, false)));
        assert_eq!(weather_levels("minecraft:thunder"), Some((true, true)));
        assert_eq!(weather_levels("snow"), None);
    }

    #[test]
    fn packet_bytes() {
        assert_eq!(game_event_body(7, 1.0), vec![7, 0x3F, 0x80, 0, 0]);
        assert_eq!(weather_events(false, false)[0], vec![2, 0, 0, 0, 0]);
        // game time 1, one clock, id 0, 6000 ticks (varlong 0xF0 0x2E), partial 0.0, rate 0.0
        assert_eq!(
            set_time_body(1, 0, 6000, 0.0),
            vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0xF0, 0x2E, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn a_lock_is_set_kept_and_lifted() {
        let mut s = Sessions::default();
        assert_eq!(s.step(1, None, None), vec![]);
        assert_eq!(s.step(1, Some("minecraft:rain"), Some("6000")), vec![Action::Weather(true, false), Action::Time(6000, false)]);
        // weather unchanged: only the time is sent again
        assert_eq!(s.step(1, Some("minecraft:rain"), Some("6000")), vec![Action::Time(6000, false)]);
        assert_eq!(s.step(1, Some("minecraft:clear"), None), vec![Action::Weather(false, false), Action::ResetTime]);
        assert_eq!(s.step(1, None, None), vec![Action::ResetWeather]);
        assert_eq!(s.step(1, None, None), vec![]);
    }
}
