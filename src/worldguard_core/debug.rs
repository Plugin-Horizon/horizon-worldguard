//! `/wg testbreak|testplace|testinteract|testdamage` (`DebuggingCommands`): the ray trace to the block or
//! entity a player looks at, the options `-t` and `-s`, and the report text (`CancelReport`).

/// How far the trace looks (`BukkitDebugHandler.MAX_TRACE_DISTANCE`).
pub const MAX_TRACE_DISTANCE: usize = 20;

/// What a `/wg test*` command was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestArgs {
    /// `-t`: run the test from the viewpoint of the named player rather than the sender.
    pub from_target: bool,
    /// `-s`: stack-trace mode (the report goes to the console and the paste service).
    pub stack_trace: bool,
    pub target: String,
}

/// `usage = "[player]"`, `min = 1`, `max = 1`, `flags = "ts"`.
pub fn parse_args(args: &str) -> Result<TestArgs, String> {
    let mut out = TestArgs { from_target: false, stack_trace: false, target: String::new() };
    let mut words = 0;
    for w in args.split_whitespace() {
        if let Some(flags) = w.strip_prefix('-').filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphabetic())) {
            for c in flags.chars() {
                match c {
                    't' => out.from_target = true,
                    's' => out.stack_trace = true,
                    other => return Err(format!("Unknown flag: {other}")),
                }
            }
        } else {
            words += 1;
            if words == 1 {
                out.target = w.to_owned();
            }
        }
    }
    match words {
        0 => Err("Too few parameters.".to_owned()),
        1 => Ok(out),
        _ => Err("Too many parameters.".to_owned()),
    }
}

/// The unit vector a player looks along (Minecraft's yaw 0 faces +z, pitch positive looks down).
pub fn look_direction(yaw: f32, pitch: f32) -> [f64; 3] {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}

/// The blocks along the ray from `eye`, each once, in order, up to `MAX_TRACE_DISTANCE` of them
/// (`BlockIterator`'s walk, at a finer step).
pub fn trace_blocks(eye: [f64; 3], dir: [f64; 3]) -> Vec<[i32; 3]> {
    let mut out: Vec<[i32; 3]> = Vec::new();
    let mut t = 0.0;
    while t <= MAX_TRACE_DISTANCE as f64 && out.len() < MAX_TRACE_DISTANCE {
        let b = [
            (eye[0] + dir[0] * t).floor() as i32,
            (eye[1] + dir[1] * t).floor() as i32,
            (eye[2] + dir[2] * t).floor() as i32,
        ];
        if out.last() != Some(&b) {
            out.push(b);
        }
        t += 0.05;
    }
    out
}

/// The report of a test: `blocked` is the reason WorldGuard gave when it refused, `None` when it let the action through.
/// (`CancelReport.toString`, which lists each plugin that cancelled; here WorldGuard is the only one asked.)
pub fn report(blocked: Option<&str>) -> Vec<String> {
    match blocked {
        Some(why) => vec![
            "Was the action blocked? YES".to_owned(),
            format!("#1 BLOCKED by WorldGuard ({why})"),
        ],
        None => vec![
            "Was the action blocked? NO".to_owned(),
            "No plugins cancelled the event. Other causes for cancellation: (1) the game may use a different hook for the action (example: buckets); or (2) the server's spawn protection has not been disabled.".to_owned(),
        ],
    }
}

/// The command that shows `playDenyEffect`'s smoke at `at` to the player `name` alone.
pub fn smoke_command(dimension: &str, name: &str, at: [f64; 3]) -> String {
    format!(
        "execute in {dimension} run particle minecraft:smoke {:.2} {:.2} {:.2} 0.15 0.15 0.15 0.02 8 normal {name}",
        at[0], at[1], at[2]
    )
}

/// One row of a profile: a name, how many times it ran and the nanoseconds it took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub calls: u64,
    pub nanos: u64,
}

/// `/wg profile` minutes to server ticks (`WorldGuardCommands.profile`: 1 to 10 minutes, 5 by default).
pub fn profile_ticks(minutes: Option<i64>) -> Result<u32, String> {
    let m = minutes.unwrap_or(5);
    if m < 1 {
        Err("You must run the profile for at least 1 minute.".to_owned())
    } else if m > 10 {
        Err("You can profile for, at maximum, 10 minutes.".to_owned())
    } else {
        Ok(m as u32 * 60 * 20)
    }
}

/// The text of `profile.txt`: each section's rows, costliest first, with the time in milliseconds.
pub fn profile_text(ticks: u32, dropped: u64, sections: &[(&str, Vec<Row>)]) -> String {
    let mut out = format!("WorldGuard profile: {ticks} ticks");
    if dropped > 0 {
        out.push_str(&format!(" ({dropped} mod calls could not be kept, the mod rows are a lower bound)"));
    }
    out.push('\n');
    for (title, rows) in sections {
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{title}\n{}\n", "-".repeat(title.len())));
        for r in rows {
            out.push_str(&format!("{:>10.3} ms  {:>8} calls  {}\n", r.nanos as f64 / 1e6, r.calls, r.name));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_take_t_and_s_and_one_player() {
        let a = parse_args("-ts Notch").unwrap();
        assert!(a.from_target && a.stack_trace && a.target == "Notch");
        assert_eq!(parse_args("Notch -s").unwrap().stack_trace, true);
        assert!(!parse_args("Notch").unwrap().from_target);
        assert_eq!(parse_args("").unwrap_err(), "Too few parameters.");
        assert_eq!(parse_args("a b").unwrap_err(), "Too many parameters.");
        assert_eq!(parse_args("-x a").unwrap_err(), "Unknown flag: x");
    }

    #[test]
    fn the_ray_walks_the_blocks_in_front_of_the_eye() {
        // yaw 0 faces +z, yaw -90 faces +x, pitch 90 looks straight down
        let d = look_direction(0.0, 0.0);
        assert!((d[2] - 1.0).abs() < 1e-9 && d[0].abs() < 1e-9);
        let d = look_direction(-90.0, 0.0);
        assert!((d[0] - 1.0).abs() < 1e-9);
        let blocks = trace_blocks([0.5, 64.5, 0.5], look_direction(0.0, 0.0));
        assert_eq!(blocks[0], [0, 64, 0]);
        assert_eq!(blocks[1], [0, 64, 1]);
        assert_eq!(blocks.len(), MAX_TRACE_DISTANCE);
        let down = trace_blocks([0.5, 64.5, 0.5], look_direction(0.0, 90.0));
        assert_eq!(down[1], [0, 63, 0]);
    }

    #[test]
    fn profile_minutes_and_text() {
        assert_eq!(profile_ticks(None), Ok(6000));
        assert_eq!(profile_ticks(Some(10)), Ok(12000));
        assert!(profile_ticks(Some(0)).is_err() && profile_ticks(Some(11)).is_err());
        let t = profile_text(
            40,
            2,
            &[
                ("Mods", vec![Row { name: "worldguard block_break".into(), calls: 3, nanos: 1_500_000 }]),
                ("Chunks", vec![]),
            ],
        );
        assert!(t.starts_with("WorldGuard profile: 40 ticks (2 mod calls could not be kept"));
        assert!(t.contains("Mods\n----\n") && t.contains("1.500 ms") && t.contains("worldguard block_break"));
        assert!(!t.contains("Chunks"));
    }

    #[test]
    fn the_deny_smoke_is_one_particle_command() {
        assert_eq!(
            smoke_command("minecraft:overworld", "Steve", [1.5, 65.0, 2.5]),
            "execute in minecraft:overworld run particle minecraft:smoke 1.50 65.00 2.50 0.15 0.15 0.15 0.02 8 normal Steve"
        );
    }

    #[test]
    fn the_report_names_the_answer() {
        assert_eq!(report(Some("break")).len(), 2);
        assert!(report(Some("break"))[0].ends_with("YES"));
        assert!(report(None)[0].ends_with("NO"));
    }
}
