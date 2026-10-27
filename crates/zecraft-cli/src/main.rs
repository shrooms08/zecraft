//! `zecraft`: the Zecraft command line.
//!
//! `zecraft new <name>` scaffolds a complete game from the coin flip (the
//! smallest working Zecraft game): a rules crate whose tests pass out of the
//! box, the three server binaries, default parameters and a README, and adds
//! both crates to the workspace.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};

mod template;

#[derive(Parser)]
#[command(
    name = "zecraft",
    version,
    about = "Zecraft: a game engine for fair, private games on shielded Zcash"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new game from the coin flip.
    New {
        /// Game name: lowercase letters and digits, starting with a letter
        /// (for example `dice`). Crates become `zecraft-<name>` and
        /// `zecraft-<name>-server`.
        name: String,
        /// Where to put it, relative to the workspace root (default `games/<name>`).
        #[arg(long)]
        path: Option<PathBuf>,
        /// The memo's 4-byte magic (default: from the name, for example `DICE`).
        #[arg(long)]
        magic: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::New { name, path, magic } => {
            let root = workspace_root(&std::env::current_dir()?)?;
            let game = template::Game::new(&name, magic.as_deref())?;
            let rel = path.unwrap_or_else(|| PathBuf::from("games").join(&name));
            let created = scaffold(&root, &rel, &game)?;
            println!("Created {} in {}:", game.name, rel.display());
            for file in &created {
                println!("  {}", file.display());
            }
            println!(
                "Added {0}/rules and {0}/server to the workspace members.",
                rel.display()
            );
            println!();
            println!("Next:");
            println!(
                "  env -u RUSTFLAGS cargo test --release -p zecraft-{}",
                game.name
            );
            println!(
                "  then edit the lines marked `YOUR GAME:` in {}/rules/src/lib.rs",
                rel.display()
            );
            println!("  (docs/BUILD_A_GAME.md walks through the rest)");
            Ok(())
        }
    }
}

/// The Zecraft workspace containing `start` (the directory whose Cargo.toml
/// has `[workspace]` and which holds crates/zecraft-core).
fn workspace_root(start: &Path) -> anyhow::Result<PathBuf> {
    for dir in start.ancestors() {
        let manifest = dir.join("Cargo.toml");
        if dir.join("crates/zecraft-core/Cargo.toml").is_file()
            && fs::read_to_string(&manifest).is_ok_and(|s| s.contains("[workspace]"))
        {
            return Ok(dir.to_path_buf());
        }
    }
    bail!("run `zecraft new` inside the Zecraft workspace (no Cargo.toml with [workspace] and crates/zecraft-core found above {})", start.display())
}

/// Writes the game under `root/rel` and adds it to the workspace.
fn scaffold(root: &Path, rel: &Path, game: &template::Game) -> anyhow::Result<Vec<PathBuf>> {
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!(
            "--path must be a plain relative path inside the workspace, got {}",
            rel.display()
        );
    }
    let dir = root.join(rel);
    if dir.exists() {
        bail!(
            "{} already exists: choose another name or --path",
            rel.display()
        );
    }
    check_collisions(root, game)?;
    let depth = rel.components().count() + 1; // rules/ and server/ sit one level below
    let up = "../".repeat(depth);
    let files = template::render(game, up.trim_end_matches('/'))?;
    let mut created = Vec::new();
    for (path, text) in &files {
        let target = dir.join(path);
        fs::create_dir_all(target.parent().expect("a file has a parent"))
            .with_context(|| format!("creating {}", target.display()))?;
        fs::write(&target, text).with_context(|| format!("writing {}", target.display()))?;
        created.push(rel.join(path));
    }
    add_members(root, rel)?;
    created.sort();
    Ok(created)
}

/// Refuses a game whose crate names or memo magic another crate or game
/// already uses.
fn check_collisions(root: &Path, game: &template::Game) -> anyhow::Result<()> {
    let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    let crates = [
        format!("zecraft-{}", game.name),
        format!("zecraft-{}-server", game.name),
    ];
    for member in members(&manifest) {
        let toml = fs::read_to_string(root.join(&member).join("Cargo.toml")).unwrap_or_default();
        for name in &crates {
            if toml.contains(&format!("name = \"{name}\"")) {
                bail!("the crate {name} already exists ({member})");
            }
        }
        let lib = fs::read_to_string(root.join(&member).join("src/lib.rs")).unwrap_or_default();
        if lib.contains(&format!("magic: *b\"{}\"", game.magic)) {
            bail!(
                "the memo magic {} is already used by {member}; pass --magic with four other ASCII characters",
                game.magic
            );
        }
    }
    Ok(())
}

/// The workspace members listed in `manifest`.
fn members(manifest: &str) -> Vec<String> {
    let Some(start) = manifest.find("members = [") else {
        return Vec::new();
    };
    let list = &manifest[start + "members = [".len()..];
    let list = &list[..list.find(']').unwrap_or(list.len())];
    list.split(',')
        .map(|m| m.trim().trim_matches('"').to_owned())
        .filter(|m| !m.is_empty())
        .collect()
}

fn add_members(root: &Path, rel: &Path) -> anyhow::Result<()> {
    let path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&path)?;
    let start = manifest
        .find("members = [")
        .context("the workspace Cargo.toml has no `members = [` list")?;
    let end = start
        + manifest[start..]
            .find(']')
            .context("unterminated members list")?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let insert = format!(", \"{rel}/rules\", \"{rel}/server\"");
    let updated = format!("{}{}{}", &manifest[..end], insert, &manifest[end..]);
    fs::write(&path, updated)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("crates/zecraft-core")).expect("core dir");
        fs::write(
            dir.path().join("crates/zecraft-core/Cargo.toml"),
            "[package]\nname = \"zecraft-core\"\n",
        )
        .expect("core");
        fs::create_dir_all(dir.path().join("games/coinflip/rules/src")).expect("game dir");
        fs::write(
            dir.path().join("games/coinflip/rules/Cargo.toml"),
            "[package]\nname = \"zecraft-coinflip\"\n",
        )
        .expect("coinflip");
        fs::write(
            dir.path().join("games/coinflip/rules/src/lib.rs"),
            "magic: *b\"ZCFL\",",
        )
        .expect("lib");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/zecraft-core\", \"games/coinflip/rules\"]\n",
        )
        .expect("manifest");
        dir
    }

    #[test]
    fn scaffolds_a_game_and_registers_it() {
        let ws = workspace();
        let game = template::Game::new("dice", None).expect("game");
        let created = scaffold(ws.path(), Path::new("games/dice"), &game).expect("scaffold");
        assert!(created.contains(&PathBuf::from("games/dice/rules/src/lib.rs")));
        let manifest = fs::read_to_string(ws.path().join("Cargo.toml")).expect("manifest");
        assert!(manifest.contains("\"games/dice/rules\", \"games/dice/server\"]"));
        let lib = fs::read_to_string(ws.path().join("games/dice/rules/src/lib.rs")).expect("lib");
        assert!(lib.contains("magic: *b\"DICE\"") && lib.contains("b\"zecraft/dice/v1\""));
        assert!(
            !lib.to_lowercase().contains("coinflip"),
            "no trace of the template's names"
        );
        let cargo =
            fs::read_to_string(ws.path().join("games/dice/rules/Cargo.toml")).expect("cargo");
        assert!(cargo.contains("path = \"../../../crates/zecraft-core\""));
        // A second time, the same name or a used magic is refused.
        assert!(scaffold(ws.path(), Path::new("games/dice"), &game).is_err());
        let clash = template::Game::new("dice2", Some("DICE")).expect("game");
        assert!(scaffold(ws.path(), Path::new("games/dice2"), &clash).is_err());
        let used = template::Game::new("zcfl", None).expect("game");
        assert!(scaffold(ws.path(), Path::new("games/zcfl"), &used).is_err());
    }

    #[test]
    fn paths_must_stay_inside_the_workspace() {
        let ws = workspace();
        let game = template::Game::new("dice", None).expect("game");
        for bad in ["../dice", "/tmp/dice", "games/../../dice"] {
            assert!(scaffold(ws.path(), Path::new(bad), &game).is_err(), "{bad}");
        }
        // A deeper path gets deeper relative paths.
        scaffold(ws.path(), Path::new("games/extra/dice"), &game).expect("scaffold");
        let cargo =
            fs::read_to_string(ws.path().join("games/extra/dice/rules/Cargo.toml")).expect("cargo");
        assert!(cargo.contains("path = \"../../../../crates/zecraft-core\""));
    }
}
