use cargo_featurex::{feature_set::Features, workspace, Package, Workspace};
use clap::Parser;
use error_stack::{IntoReport, ResultExt};
use itertools::Itertools;
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
		Some(Subcommand::Check) => run_permutations(workspace, "check", stdout),
		Some(Subcommand::Test) => run_permutations(workspace, "test", stdout),
		Some(Subcommand::Clippy) => run_permutations(workspace, "clippy", stdout),
		Some(Subcommand::Build) => run_permutations(workspace, "build", stdout),
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

fn run_permutations(
	workspace: Workspace,
	command: &str,
	mut out: StandardStream,
) -> error_stack::Result<(), FeaturexError> {
	for pkg in workspace.packages() {
		for permutation in pkg.features.permutations() {
			run(pkg, permutation, command, &mut out)?;
		}
	}

	Ok(())
}

fn run(
	pkg: &Package,
	features: Features,
	command: &str,
	out: &mut StandardStream,
) -> error_stack::Result<(), FeaturexError> {
	let mut cmd = Command::new("cargo");
	cmd
		.arg(command)
		.arg("--manifest-path")
		.arg(pkg.manifest_path())
		.arg("--features")
		.arg(features.iter().map(|f| f.name().to_owned()).join(","))
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());

	let features = features.iter().map(|f| f.name().to_owned()).join(", ");

	out
		.set_color(ColorSpec::new().set_fg(Some(Color::Magenta)))
		.into_report()
		.change_context(FeaturexError)?;
	writeln!(out, "    ========== {}[{}] ==========", pkg.name, features)
		.into_report()
		.change_context(FeaturexError)?;
	out.reset().into_report().change_context(FeaturexError)?;

	cmd
		.output()
		.map(|_| ())
		.into_report()
		.change_context(FeaturexError)
}
