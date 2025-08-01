use cargo_featurex::{Package, PackageInfo, Workspace, feature_set::Features, workspace};
use clap::Parser;
use core::fmt;
use error_stack::{ResultExt, ensure};
use itertools::Itertools;
use serde_json::{Value, json};
use std::{
	io::Write,
	process::{Command, Stdio},
};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use thiserror::Error;

#[derive(Parser)] // requires `derive` feature
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
enum Cargo {
	Featurex(Featurex),
}

#[derive(clap::Args, Debug)]
#[command(author, version, about, long_about = None)]
struct Featurex {
	#[arg(long)]
	manifest_path: Option<std::path::PathBuf>,

	#[clap(subcommand)]
	subcommand: Option<Subcommand>,
}

#[derive(clap::Subcommand, Debug)]
enum Subcommand {
	Json,
	Test,
	Check,
	Clippy,
	Build,
}

#[derive(Debug, Error)]
#[error("program failed")]
struct FeaturexError;

fn main() -> error_stack::Result<(), FeaturexError> {
	let Cargo::Featurex(args) = Cargo::parse();
	let stdout = StandardStream::stdout(ColorChoice::Auto);

	let workspace = workspace(args.manifest_path.as_deref()).change_context(FeaturexError)?;
	match args.subcommand {
		None => print_permutations(workspace),
		Some(Subcommand::Json) => print_permutations_json(workspace),
		Some(Subcommand::Check) => {
			run_permutations(workspace, "check", stdout).change_context(FeaturexError)
		}
		Some(Subcommand::Test) => {
			run_permutations(workspace, "test", stdout).change_context(FeaturexError)
		}
		Some(Subcommand::Clippy) => {
			run_permutations(workspace, "clippy", stdout).change_context(FeaturexError)
		}
		Some(Subcommand::Build) => {
			run_permutations(workspace, "build", stdout).change_context(FeaturexError)
		}
	}
}

fn print_permutations(workspace: Workspace) -> error_stack::Result<(), FeaturexError> {
	for pkg in workspace.packages() {
		for permutation in pkg.features.permutations() {
			let features = permutation
				.into_iter()
				.map(|f| f.name().to_owned())
				.join(", ");

			println!("{} [{}]", pkg.id, features);
		}
	}

	Ok(())
}

fn print_permutations_json(workspace: Workspace) -> error_stack::Result<(), FeaturexError> {
	let packages = workspace
		.packages()
		.iter()
		.map(|pkg| {
			let feautres = pkg
				.features
				.features()
				.map(|f| Value::from(f.name()))
				.collect::<Value>();
			let permutations = pkg
				.features
				.permutations()
				.map(|p| {
					p.into_iter()
						.map(|f| Value::from(f.name()))
						.collect::<Value>()
				})
				.collect::<Value>();

			json!({
				"id": pkg.id(),
				"name": pkg.name(),
				"version": pkg.version(),
				"manifest_path": pkg.manifest_path(),
				"features": {
					"all": feautres,
					"permutations": permutations,
				}
			})
		})
		.collect::<Value>();

	let json = json!({
		"packages": packages,
	});

	println!("{json}");
	Ok(())
}

#[derive(Debug, Error)]
struct PermutationError {
	package: PackageInfo,
	features: Vec<String>,
}

impl fmt::Display for PermutationError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"failed to run permutation for package {} with features: [{}]",
			self.package.name,
			self.features.join(", ")
		)
	}
}

fn run_permutations(
	workspace: Workspace,
	command: &str,
	mut out: StandardStream,
) -> error_stack::Result<(), PermutationError> {
	for pkg in workspace.packages() {
		for permutation in pkg.features.permutations() {
			run(pkg, &permutation, command, &mut out).change_context_lazy(|| PermutationError {
				package: pkg.info(),
				features: permutation.iter().map(|f| f.name().to_owned()).collect(),
			})?;
		}
	}

	Ok(())
}

#[derive(Debug, Error)]
enum RunError {
	#[error("failed to print logs")]
	LogError,

	#[error("process falied to start")]
	StartProcess,

	#[error("process failed with status code: {0}")]
	ProcessFailed(i32),
}

fn run(
	pkg: &Package,
	features: &Features,
	command: &str,
	out: &mut StandardStream,
) -> error_stack::Result<(), RunError> {
	let mut cmd = Command::new("cargo");
	cmd
		.arg(command)
		.arg("--manifest-path")
		.arg(pkg.manifest_path())
		.arg("--no-default-features")
		.arg("--features")
		.arg(features.iter().map(|f| f.name().to_owned()).join(","))
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());

	let features = features.iter().map(|f| f.name().to_owned()).join(", ");

	out
		.set_color(ColorSpec::new().set_fg(Some(Color::Magenta)))
		.change_context(RunError::LogError)?;
	writeln!(out, "    ========== {}[{}] ==========", pkg.name, features)
		.change_context(RunError::LogError)?;
	out.reset().change_context(RunError::LogError)?;

	let result = cmd.output().change_context(RunError::StartProcess)?;

	ensure!(
		result.status.success(),
		RunError::ProcessFailed(result.status.code().unwrap_or(-1))
	);

	Ok(())
}
