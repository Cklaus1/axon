//! `axon-surface` CLI entry point.

use anyhow::Result as AnyResult;
use axon_surface::{compile, parser::GoalFile};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "axon-surface",
    version,
    about = "Phase-10 prose-to-AST compiler — turns goal.md into goal.ax"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Validate a goal file: parses required sections + extracts
    /// inputs/outputs + verify predicate.  Exit-code-clean on
    /// success; non-zero with diagnostic on failure.
    Validate {
        /// Path to the goal `.md` file.
        input: PathBuf,
    },
    /// Compile a goal file into a `.ax` skeleton.
    Compile {
        /// Path to the goal `.md` file.
        input: PathBuf,
        /// Output path for the `.ax` skeleton.  Defaults to the
        /// input path with the extension swapped to `.ax`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> AnyResult<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Validate { input } => {
            let md = std::fs::read_to_string(&input)?;
            let goal =
                GoalFile::parse(&md).map_err(|e| anyhow::anyhow!("validation failed: {e}"))?;
            println!("✓ goal file is valid: `{}`", input.display());
            println!("  title:    {}", goal.title);
            println!("  inputs:   {:?}", goal.inputs()?);
            println!("  outputs:  {:?}", goal.outputs()?);
            println!("  verify:   {}", goal.verify_predicate()?);
            println!(
                "  sections: {} (all 10 required present)",
                goal.sections.len()
            );
            Ok(())
        }
        Cmd::Compile { input, out } => {
            let md = std::fs::read_to_string(&input)?;
            let goal = GoalFile::parse(&md).map_err(|e| anyhow::anyhow!("parse failed: {e}"))?;
            let ax = compile::emit(&goal).map_err(|e| anyhow::anyhow!("emit failed: {e}"))?;
            let out_path = out.unwrap_or_else(|| input.with_extension("ax"));
            std::fs::write(&out_path, &ax)?;
            println!("wrote {} ({} bytes)", out_path.display(), ax.len());
            Ok(())
        }
    }
}
