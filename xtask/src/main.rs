use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const DEFAULT_MUTANTS_DIR: &str = "target/mutants";
const DEFAULT_MUTATION_THRESHOLD: u32 = 50;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => exit_code(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<i32, String> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(1);
    };

    match command {
        "mutants" => run_mutants(args.get(1).map_or(DEFAULT_MUTANTS_DIR, String::as_str)),
        "mutants-gate" => mutants_gate(
            parse_threshold(args.get(1).map(String::as_str))?,
            args.get(2).map_or(DEFAULT_MUTANTS_DIR, String::as_str),
        ),
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(0)
        }
        unknown => Err(format!("unknown xtask command: {unknown}")),
    }
}

fn run_mutants(output_dir: &str) -> Result<i32, String> {
    let cache_dir = env::var_os("LUCIDE_STATIC_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or(
            env::current_dir()
                .map_err(|err| format!("resolving current directory: {err}"))?
                .join("target")
                .join("lucide-static-cache"),
        );

    let status = Command::new("cargo")
        .args([
            "mutants",
            "--workspace",
            "--copy-target",
            "false",
            "--output",
            output_dir,
            "--",
            "--lib",
            "--bins",
        ])
        .env("LUCIDE_STATIC_CACHE_DIR", cache_dir)
        .status()
        .map_err(|err| format!("running cargo mutants: {err}"))?;

    match status.code() {
        Some(0 | 2) => Ok(0),
        Some(code) => Ok(code),
        None => Ok(1),
    }
}

fn mutants_gate(threshold: u32, output_dir: &str) -> Result<i32, String> {
    let mutants_status = run_mutants(output_dir)?;
    let outcomes_path = Path::new(output_dir)
        .join("mutants.out")
        .join("outcomes.json");

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
    let summary = mutation_summary(&outcomes);

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

fn parse_threshold(value: Option<&str>) -> Result<u32, String> {
    value.map_or(Ok(DEFAULT_MUTATION_THRESHOLD), |value| {
        value
            .parse()
            .map_err(|err| format!("invalid mutation threshold {value:?}: {err}"))
    })
}

fn mutation_summary(outcomes: &str) -> MutationSummary {
    MutationSummary {
        caught: count_summary(outcomes, "CaughtMutant"),
        missed: count_summary(outcomes, "MissedMutant"),
        timeout: count_summary(outcomes, "Timeout"),
    }
}

fn count_summary(outcomes: &str, summary: &str) -> u32 {
    outcomes
        .matches(&format!(r#""summary": "{summary}""#))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn print_usage() {
    println!(
        "usage:
  cargo run -p xtask -- mutants [output-dir]
  cargo run -p xtask -- mutants-gate [threshold] [output-dir]"
    );
}

fn exit_code(code: i32) -> ExitCode {
    u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from)
}

#[derive(Debug, Eq, PartialEq)]
struct MutationSummary {
    caught: u32,
    missed: u32,
    timeout: u32,
}

impl MutationSummary {
    fn total(&self) -> u32 {
        self.caught + self.missed + self.timeout
    }

    fn score(&self) -> u32 {
        self.caught * 100 / self.total()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_viable_mutant_summaries() {
        let outcomes = r#"
          {"summary": "CaughtMutant"}
          {"summary": "MissedMutant"}
          {"summary": "Timeout"}
          {"summary": "Unviable"}
          {"summary": "CaughtMutant"}
        "#;

        assert_eq!(
            mutation_summary(outcomes),
            MutationSummary {
                caught: 2,
                missed: 1,
                timeout: 1,
            }
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
    fn parses_default_mutation_threshold() {
        assert_eq!(parse_threshold(None), Ok(DEFAULT_MUTATION_THRESHOLD));
    }
}
