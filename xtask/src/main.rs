use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand, ValueEnum, error::ErrorKind};
use serde_json::Value;

const DEFAULT_MUTANTS_DIR: &str = "target/mutants";
const DEFAULT_MUTATION_THRESHOLD: u32 = 50;
const DEFAULT_NIGHTLY_FUZZ_SECONDS: u32 = 300;
const JAVA_RUNTIME_METADATA_CORPUS: &str = "fuzz/corpus/java_runtime_metadata";
const JAVA_RUNTIME_METADATA_SEEDS: &str = "fuzz/seeds/java_runtime_metadata";

fn main() -> ExitCode {
    match run_from(env::args_os()) {
        Ok(code) => exit_code(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_from<I, T>(args: I) -> Result<i32, String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            err.print()
                .map_err(|print_err| format!("printing clap help: {print_err}"))?;
            return Ok(0);
        }
        Err(err) => return Err(err.to_string()),
    };
    run(cli)
}

fn run(cli: Cli) -> Result<i32, String> {
    match cli.command {
        CommandKind::Fuzz { command } => run_fuzz(command),
        CommandKind::Mutants { output_dir } => run_mutants(&output_dir),
        CommandKind::MutantsGate {
            threshold,
            output_dir,
        } => mutants_gate(threshold, &output_dir),
    }
}

#[derive(Debug, Parser)]
#[command(author, version, about = "Mado workspace maintenance tasks")]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Debug, Subcommand)]
enum CommandKind {
    Fuzz {
        #[command(subcommand)]
        command: FuzzCommand,
    },
    Mutants {
        #[arg(default_value = DEFAULT_MUTANTS_DIR)]
        output_dir: PathBuf,
    },
    MutantsGate {
        #[arg(default_value_t = DEFAULT_MUTATION_THRESHOLD)]
        threshold: u32,
        #[arg(default_value = DEFAULT_MUTANTS_DIR)]
        output_dir: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum FuzzCommand {
    Smoke,
    Nightly,
    Run {
        #[arg(value_enum, default_value_t = FuzzTarget::IconName)]
        target: FuzzTarget,
        #[arg(long)]
        seconds: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
enum FuzzTarget {
    IconName,
    JavaRuntimeMetadata,
}

impl FuzzTarget {
    const ALL: &'static [Self] = &[Self::IconName, Self::JavaRuntimeMetadata];

    const fn cargo_name(self) -> &'static str {
        match self {
            Self::IconName => "icon_name",
            Self::JavaRuntimeMetadata => "java_runtime_metadata",
        }
    }

    const fn corpus_paths(self) -> &'static [&'static str] {
        match self {
            Self::IconName => &[],
            Self::JavaRuntimeMetadata => {
                &[JAVA_RUNTIME_METADATA_CORPUS, JAVA_RUNTIME_METADATA_SEEDS]
            }
        }
    }

    const fn smoke_args(self) -> &'static [&'static str] {
        match self {
            Self::IconName | Self::JavaRuntimeMetadata => &["-runs=256"],
        }
    }

    const fn run_args(self) -> &'static [&'static str] {
        match self {
            Self::IconName => &[],
            Self::JavaRuntimeMetadata => &["-max_len=131072"],
        }
    }

    const fn nightly_seconds(self) -> u32 {
        match self {
            Self::IconName | Self::JavaRuntimeMetadata => DEFAULT_NIGHTLY_FUZZ_SECONDS,
        }
    }
}

fn run_fuzz(command: FuzzCommand) -> Result<i32, String> {
    match command {
        FuzzCommand::Smoke => {
            for target in FuzzTarget::ALL.iter().copied() {
                let code = run_fuzz_target(target, target.smoke_args())?;
                if code != 0 {
                    return Ok(code);
                }
            }
            Ok(0)
        }
        FuzzCommand::Nightly => {
            for target in FuzzTarget::ALL.iter().copied() {
                let mut args = target.run_args().to_vec();
                let max_total_time = format!("-max_total_time={}", target.nightly_seconds());
                args.push(&max_total_time);

                let code = run_fuzz_target(target, &args)?;
                if code != 0 {
                    return Ok(code);
                }
            }
            Ok(0)
        }
        FuzzCommand::Run { target, seconds } => {
            let mut args = target.run_args().to_vec();
            let max_total_time;
            if let Some(seconds) = seconds {
                max_total_time = format!("-max_total_time={seconds}");
                args.push(&max_total_time);
            }
            run_fuzz_target(target, &args)
        }
    }
}

fn run_fuzz_target(target: FuzzTarget, libfuzzer_args: &[&str]) -> Result<i32, String> {
    let mut command = Command::new("cargo");
    command
        .arg("+nightly")
        .arg("fuzz")
        .arg("run")
        .arg(target.cargo_name());

    for path in target.corpus_paths() {
        command.arg(path);
    }

    if !libfuzzer_args.is_empty() {
        command.arg("--");
        command.args(libfuzzer_args);
    }

    command_status(
        command,
        &format!("running fuzz target {}", target.cargo_name()),
    )
}

fn run_mutants(output_dir: &Path) -> Result<i32, String> {
    let cache_dir = env::var_os("LUCIDE_STATIC_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or(
            env::current_dir()
                .map_err(|err| format!("resolving current directory: {err}"))?
                .join("target")
                .join("lucide-static-cache"),
        );

    let mut command = Command::new("cargo");
    command
        .args([
            "mutants",
            "--workspace",
            "--copy-target",
            "false",
            "--output",
        ])
        .arg(output_dir)
        .args(["--", "--lib", "--bins"])
        .env("LUCIDE_STATIC_CACHE_DIR", cache_dir);

    match command_status(command, "running cargo mutants")? {
        0 | 2 => Ok(0),
        code => Ok(code),
    }
}

fn mutants_gate(threshold: u32, output_dir: &Path) -> Result<i32, String> {
    let mutants_status = run_mutants(output_dir)?;
    let outcomes_path = output_dir.join("mutants.out").join("outcomes.json");

    if !outcomes_path.is_file() {
        eprintln!(
            "missing cargo-mutants outcomes file: {}",
            outcomes_path.display()
        );
        return Ok(if mutants_status == 0 {
            1
        } else {
            mutants_status
        });
    }

    let outcomes = fs::read_to_string(&outcomes_path)
        .map_err(|err| format!("reading {}: {err}", outcomes_path.display()))?;
    let summary = mutation_summary(&outcomes)?;

    if summary.total() == 0 {
        println!("no viable mutants were tested");
        return Ok(1);
    }

    let score = summary.score();
    println!(
        "mutation score: {score}% ({}/{}) caught",
        summary.caught,
        summary.total()
    );

    if score < threshold {
        eprintln!("mutation score {score}% is below required {threshold}%");
        return Ok(1);
    }

    Ok(0)
}

fn command_status(mut command: Command, context: &str) -> Result<i32, String> {
    let status = command
        .status()
        .map_err(|err| format!("{context}: {err}"))?;

    Ok(status.code().unwrap_or(1))
}

fn mutation_summary(outcomes: &str) -> Result<MutationSummary, String> {
    let value: Value = serde_json::from_str(outcomes)
        .map_err(|err| format!("parsing cargo-mutants JSON: {err}"))?;
    let Some(outcomes) = value
        .as_array()
        .or_else(|| value.get("outcomes").and_then(Value::as_array))
    else {
        return Err("cargo-mutants outcomes JSON must contain an outcomes array".to_string());
    };

    let mut summary = MutationSummary::default();
    for outcome in outcomes {
        match outcome.get("summary").and_then(Value::as_str) {
            Some("CaughtMutant") => summary.caught += 1,
            Some("MissedMutant") => summary.missed += 1,
            Some("Timeout") => summary.timeout += 1,
            _ => {}
        }
    }

    Ok(summary)
}

fn exit_code(code: i32) -> ExitCode {
    u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from)
}

#[derive(Debug, Default, Eq, PartialEq)]
struct MutationSummary {
    caught: u32,
    missed: u32,
    timeout: u32,
}

impl MutationSummary {
    const fn total(&self) -> u32 {
        self.caught + self.missed + self.timeout
    }

    const fn score(&self) -> u32 {
        self.caught * 100 / self.total()
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn counts_only_viable_mutant_summaries() {
        let outcomes = r#"
        {
          "outcomes": [
            {"summary": "CaughtMutant"},
            {"summary": "MissedMutant"},
            {"summary": "Timeout"},
            {"summary": "Unviable"},
            {"summary": "CaughtMutant"}
          ]
        }
        "#;

        assert_eq!(
            mutation_summary(outcomes),
            Ok(MutationSummary {
                caught: 2,
                missed: 1,
                timeout: 1,
            })
        );
    }

    #[test]
    fn rejects_malformed_mutation_summary_json() {
        let error = mutation_summary("not json").unwrap_err();

        assert!(error.contains("parsing cargo-mutants JSON"));
    }

    #[test]
    fn accepts_legacy_top_level_array_mutation_summary_json() {
        assert_eq!(
            mutation_summary(r#"[{"summary":"CaughtMutant"}]"#),
            Ok(MutationSummary {
                caught: 1,
                missed: 0,
                timeout: 0,
            })
        );
    }

    #[test]
    fn rejects_missing_mutation_outcomes_array() {
        assert_eq!(
            mutation_summary(r#"{"summary":"CaughtMutant"}"#),
            Err("cargo-mutants outcomes JSON must contain an outcomes array".to_string())
        );
    }

    #[test]
    fn score_uses_integer_percentage_like_the_shell_gate() {
        let summary = MutationSummary {
            caught: 2,
            missed: 1,
            timeout: 1,
        };

        assert_eq!(summary.score(), 50);
    }

    #[test]
    fn parses_mutants_command_defaults() {
        let cli = Cli::try_parse_from(["xtask", "mutants"]).unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::Mutants { ref output_dir } if output_dir == Path::new(DEFAULT_MUTANTS_DIR)
        ));
    }

    #[test]
    fn parses_mutants_gate_defaults() {
        let cli = Cli::try_parse_from(["xtask", "mutants-gate"]).unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::MutantsGate {
                threshold: DEFAULT_MUTATION_THRESHOLD,
                ref output_dir,
            } if output_dir == Path::new(DEFAULT_MUTANTS_DIR)
        ));
    }

    #[test]
    fn parses_custom_mutation_threshold() {
        let cli = Cli::try_parse_from(["xtask", "mutants-gate", "75", "target/custom"]).unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::MutantsGate {
                threshold: 75,
                ref output_dir,
            } if output_dir == Path::new("target/custom")
        ));
    }

    #[test]
    fn reports_invalid_mutation_threshold() {
        let error = Cli::try_parse_from(["xtask", "mutants-gate", "high"]).unwrap_err();

        assert!(error.to_string().contains("invalid digit"));
    }

    #[test]
    fn parses_fuzz_smoke_command() {
        let cli = Cli::try_parse_from(["xtask", "fuzz", "smoke"]).unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::Fuzz {
                command: FuzzCommand::Smoke
            }
        ));
    }

    #[test]
    fn parses_fuzz_nightly_command() {
        let cli = Cli::try_parse_from(["xtask", "fuzz", "nightly"]).unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::Fuzz {
                command: FuzzCommand::Nightly
            }
        ));
    }

    #[test]
    fn registered_fuzz_targets_match_cli_variants() {
        assert_eq!(FuzzTarget::ALL, FuzzTarget::value_variants());
    }

    #[test]
    fn parses_fuzz_run_defaults() {
        let cli = Cli::try_parse_from(["xtask", "fuzz", "run"]).unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::Fuzz {
                command: FuzzCommand::Run {
                    target: FuzzTarget::IconName,
                    seconds: None,
                }
            }
        ));
    }

    #[test]
    fn parses_fuzz_run_java_runtime_metadata() {
        let cli = Cli::try_parse_from([
            "xtask",
            "fuzz",
            "run",
            "java-runtime-metadata",
            "--seconds",
            "300",
        ])
        .unwrap();

        assert!(matches!(
            cli.command,
            CommandKind::Fuzz {
                command: FuzzCommand::Run {
                    target: FuzzTarget::JavaRuntimeMetadata,
                    seconds: Some(300),
                }
            }
        ));
    }

    #[test]
    fn run_reports_usage_failure_when_command_is_missing() {
        assert!(run_from(["xtask"]).is_err());
    }

    #[test]
    fn clap_reports_help_as_display_help() {
        let error = Cli::try_parse_from(["xtask", "--help"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn run_rejects_unknown_commands() {
        assert!(run_from(["xtask", "wat"]).is_err());
    }

    #[test]
    fn summary_total_includes_all_viable_outcomes() {
        let summary = MutationSummary {
            caught: 3,
            missed: 2,
            timeout: 1,
        };

        assert_eq!(summary.total(), 6);
    }

    #[test]
    fn exit_code_rejects_values_outside_u8_range() {
        assert_eq!(exit_code(0), ExitCode::SUCCESS);
        assert_eq!(exit_code(1), ExitCode::FAILURE);
        assert_eq!(exit_code(256), ExitCode::FAILURE);
        assert_eq!(exit_code(-1), ExitCode::FAILURE);
    }

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
