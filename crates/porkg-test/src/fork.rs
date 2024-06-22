use std::{
    ffi::OsString,
    process::{Command, ExitCode, Stdio},
};

pub fn in_host() -> bool {
    std::env::var_os("PORKG_IN_TEST").is_some()
}

// From RustyFork
#[derive(Clone, Copy, Debug, PartialEq)]
enum FlagType {
    /// Pass the flag through unchanged. The boolean indicates whether the flag
    /// is followed by an argument.
    Pass(bool),
    /// Drop the flag entirely. The boolean indicates whether the flag is
    /// followed by an argument.
    Drop(bool),
    /// Indicates a known flag that should never be encountered. The string is
    /// a human-readable error message.
    Error(&'static str),
}

static KNOWN_FLAGS: &[(&str, FlagType)] = &[
    ("--bench", FlagType::Pass(false)),
    ("--color", FlagType::Pass(true)),
    ("--ensure-time", FlagType::Drop(false)),
    ("--exact", FlagType::Drop(false)),
    ("--exclude-should-panic", FlagType::Pass(false)),
    ("--force-run-in-process", FlagType::Pass(false)),
    ("--format", FlagType::Drop(true)),
    (
        "--help",
        FlagType::Error("Tests run but --help passed to process?"),
    ),
    ("--ignored", FlagType::Pass(false)),
    ("--include-ignored", FlagType::Pass(false)),
    (
        "--list",
        FlagType::Error("Tests run but --list passed to process?"),
    ),
    ("--logfile", FlagType::Drop(true)),
    ("--nocapture", FlagType::Drop(false)),
    ("--quiet", FlagType::Drop(false)),
    ("--report-time", FlagType::Drop(false)),
    ("--show-output", FlagType::Pass(false)),
    ("--skip", FlagType::Drop(true)),
    ("--test", FlagType::Pass(false)),
    ("--test-threads", FlagType::Drop(true)),
    ("-Z", FlagType::Pass(true)),
    ("-h", FlagType::Error("Tests run but -h passed to process?")),
    ("-q", FlagType::Drop(false)),
];

fn get_args() -> Vec<OsString> {
    let mut result = Vec::new();
    let mut args = std::env::args_os();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }

        let item = KNOWN_FLAGS
            .iter()
            .filter_map(|(k, v)| if *k == arg { Some(v) } else { None })
            .next();

        match item {
            Some(FlagType::Pass(param)) => {
                result.push(arg.clone());
                if *param {
                    result.push(
                        args.next()
                            .unwrap_or_else(|| panic!("argument expected for {:?}", param)),
                    );
                }
            }
            Some(FlagType::Drop(param)) => {
                if *param {
                    args.next()
                        .unwrap_or_else(|| panic!("argument expected for {:?}", param));
                }
            }
            Some(FlagType::Error(err)) => {
                panic!("{}", err);
            }
            None => {}
        }
    }
    result
}

pub fn run(module: &str, test: &str) -> ExitCode {
    let exe = std::env::current_exe().expect("get the current executable");
    let module = if let Some(index) = module.find("::") {
        &module[(index + 2)..]
    } else {
        ""
    };
    let mut args = get_args();
    args.extend(
        [
            "--quiet",
            "--test-threads",
            "1",
            "--nocapture",
            "--exact",
            "--",
            &format!("{module}::{test}"),
        ]
        .map(Into::into),
    );
    let mut child = Command::new(exe)
        .args(args)
        .env("PORKG_IN_TEST", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("test process executes");
    if child.wait().expect("wait for test process").success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
