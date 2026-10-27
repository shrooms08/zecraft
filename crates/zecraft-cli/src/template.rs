//! The game template: the coin flip's own source files (embedded at build
//! time, so the template is always a game that compiles and passes its
//! tests) with its names replaced by the new game's.

use std::path::PathBuf;

use anyhow::bail;
use zecraft_core::draw::{uniform_below, Randomness};

const RULES_CARGO: &str = include_str!("../../../games/coinflip/rules/Cargo.toml");
const RULES_LIB: &str = include_str!("../../../games/coinflip/rules/src/lib.rs");
const RULES_TESTS: &str = include_str!("../../../games/coinflip/rules/tests/coinflip.rs");
const SERVER_CARGO: &str = include_str!("../../../games/coinflip/server/Cargo.toml");
const BIN_ENGINE: &str =
    include_str!("../../../games/coinflip/server/src/bin/zecraft-coinflip-engine.rs");
const BIN_KEYHOLDER: &str =
    include_str!("../../../games/coinflip/server/src/bin/zecraft-coinflip-keyholder.rs");
const BIN_VERIFY: &str =
    include_str!("../../../games/coinflip/server/src/bin/zecraft-coinflip-verify.rs");
const GAME_PARAMS: &str = include_str!("../../../games/coinflip/game-params.json");

/// Names a game is not allowed to take.
const RESERVED: &[&str] = &[
    "zecraft",
    "core",
    "vault",
    "engine",
    "keyholder",
    "verify",
    "player",
    "cli",
    "server",
    "rules",
    "wasm",
    "web",
    "test",
    "tests",
    "std",
    "raffle",
    "coinflip",
];

/// A new game's names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    /// `dice`: crate `zecraft-dice`, binaries `zecraft-dice-engine`, ...
    pub name: String,
    /// `Dice`: the Rules type (`Dice`, `DiceParams`, `DiceOutcome`).
    pub type_name: String,
    /// `DICE`: the memo's four magic bytes.
    pub magic: String,
    /// `dice1:`: the text memo prefix.
    pub prefix: String,
}

impl Game {
    /// Validates `name` (and `magic`, if given) and derives the rest.
    pub fn new(name: &str, magic: Option<&str>) -> anyhow::Result<Self> {
        let valid = name.len() >= 2
            && name.len() <= 24
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !valid {
            bail!("game names are 2 to 24 lowercase letters and digits, starting with a letter (got `{name}`)");
        }
        if RESERVED.contains(&name) {
            bail!("`{name}` is reserved or already a Zecraft game; choose another name");
        }
        let magic = match magic {
            Some(m) => {
                if m.len() != 4 || !m.chars().all(|c| c.is_ascii_alphanumeric()) {
                    bail!("--magic must be exactly four ASCII letters or digits (got `{m}`)");
                }
                m.to_owned()
            }
            None => {
                let mut m: String = name.to_ascii_uppercase().chars().take(4).collect();
                while m.len() < 4 {
                    m.push('X');
                }
                m
            }
        };
        let mut type_name = name.to_owned();
        type_name[..1].make_ascii_uppercase();
        Ok(Game {
            prefix: format!("{}1:", magic.to_ascii_lowercase()),
            name: name.to_owned(),
            type_name,
            magic,
        })
    }

    fn domain(&self) -> String {
        format!("zecraft/{}/v1", self.name)
    }

    /// The coin flip's names replaced by this game's, in every file.
    fn rename(&self, text: &str) -> String {
        let snake = self.name.replace('-', "_");
        text.replace("zecraft-coinflip", &format!("zecraft-{}", self.name))
            .replace("zecraft_coinflip", &format!("zecraft_{snake}"))
            .replace("CoinFlipParams", &format!("{}Params", self.type_name))
            .replace("FlipOutcome", &format!("{}Outcome", self.type_name))
            .replace("CoinFlip", &self.type_name)
            .replace("ZCFL", &self.magic)
            .replace("zcfl1:", &self.prefix)
            .replace("zecraft/coinflip/", &format!("zecraft/{}/", self.name))
            .replace("\"coinflip\"", &format!("\"{}\"", self.name))
            .replace("the coin flip's", &format!("{}'s", self.name))
            .replace("The coin flip's", &format!("{}'s", self.type_name))
            .replace("The coin flip", &self.type_name)
            .replace("the coin flip", &self.name)
            .replace("coin flip", &self.name)
    }

    /// The draw known answers of this game's domain, as the template's test
    /// pins them (rounds 0..8, randomness 0x5a…5a, two sides).
    fn known_answers(&self) -> anyhow::Result<String> {
        let r = Randomness([0x5a; 32]);
        let draws = (0..8)
            .map(|round| {
                uniform_below(self.domain().as_bytes(), round, &r, 2).map(|v| v.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(format!(
            "const KAT: [u32; 8] = [{}]; // zecraft-new: KAT",
            draws.join(", ")
        ))
    }
}

/// Replaces the line ending in `// zecraft-new: KAT`.
fn with_known_answers(tests: &str, line: &str) -> anyhow::Result<String> {
    let mut found = false;
    let out: Vec<String> = tests
        .lines()
        .map(|l| {
            if l.trim_end().ends_with("// zecraft-new: KAT") {
                found = true;
                line.to_owned()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        bail!("the template's tests have no `// zecraft-new: KAT` line");
    }
    Ok(out.join("\n") + "\n")
}

/// The rules crate's header comment for a new game.
fn lib_header(game: &Game) -> String {
    format!(
        "//! `zecraft-{name}`: {name}, a game built with Zecraft.\n\
         //!\n\
         //! Generated by `zecraft new {name}` from the coin flip, so it plays like\n\
         //! a coin flip until you change it: players stake exactly `stake_zat` on\n\
         //! side 0 or 1, one drand draw picks a side after the round closes, and\n\
         //! that side splits the pot. Lines marked `YOUR GAME:` are the ones to\n\
         //! change; docs/BUILD_A_GAME.md explains the three functions.\n",
        name = game.name
    )
}

fn readme(game: &Game, up: &str) -> String {
    format!(
        "# {type_name}\n\n\
         A Zecraft game, generated by `zecraft new {name}` from the coin flip.\n\n\
         | | |\n|---|---|\n\
         | Memo | `0xFF \\| \"{magic}\" \\| 1 \\| round \\| payload \\| sealed payout address`, or text `{prefix}` + base64url |\n\
         | Draw domain | `{domain}` |\n\
         | Crates | `zecraft-{name}` (rules), `zecraft-{name}-server` (binaries) |\n\
         | Binaries | `zecraft-{name}-engine`, `zecraft-{name}-keyholder`, `zecraft-{name}-verify` |\n\n\
         - `rules/src/lib.rs`: your game. Change the lines marked `YOUR GAME:`.\n\
         - `rules/tests/`: classification, draw known answers, conservation property test.\n\
         - `game-params.json`: the default parameters written into `config/{name}.testnet.json`.\n\n\
         ```sh\n\
         env -u RUSTFLAGS cargo test --release -p zecraft-{name}\n\
         GAME={name} scripts/local/keys.sh\n\
         GAME={name} scripts/local/game-config.sh <genesis height>\n\
         GAME={name} scripts/local/up.sh\n\
         ```\n\n\
         See `{up}/docs/BUILD_A_GAME.md`.\n",
        type_name = game.type_name,
        name = game.name,
        magic = game.magic,
        prefix = game.prefix,
        domain = game.domain(),
    )
}

/// Every file of the new game, relative to its directory. `up` is the path
/// from `rules/` (or `server/`) back to the workspace root, `../../..` for
/// `games/<name>`.
pub fn render(game: &Game, up: &str) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let fix_paths = |text: String| text.replace("../../../crates/", &format!("{up}/crates/"));
    let body = RULES_LIB
        .find("#![forbid(unsafe_code)]")
        .map_or(RULES_LIB, |i| &RULES_LIB[i..]);
    let lib = format!("{}\n{}", lib_header(game), game.rename(body));
    let tests = with_known_answers(&game.rename(RULES_TESTS), &game.known_answers()?)?;
    let bin = |kind: &str, text: &str| {
        (
            PathBuf::from(format!("server/src/bin/zecraft-{}-{kind}.rs", game.name)),
            game.rename(text),
        )
    };
    let readme_up = up.rsplit_once('/').map_or("..", |(rest, _)| rest);
    Ok(vec![
        (
            PathBuf::from("rules/Cargo.toml"),
            fix_paths(game.rename(RULES_CARGO)),
        ),
        (PathBuf::from("rules/src/lib.rs"), lib),
        (
            PathBuf::from(format!("rules/tests/{}.rs", game.name)),
            tests,
        ),
        (
            PathBuf::from("server/Cargo.toml"),
            fix_paths(game.rename(SERVER_CARGO)),
        ),
        bin("engine", BIN_ENGINE),
        bin("keyholder", BIN_KEYHOLDER),
        bin("verify", BIN_VERIFY),
        (PathBuf::from("game-params.json"), GAME_PARAMS.to_owned()),
        (PathBuf::from("README.md"), readme(game, readme_up)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_validated() {
        for bad in [
            "",
            "d",
            "Dice",
            "dice-roll",
            "9lives",
            "raffle",
            "coinflip",
            "zecraft",
            "a_b",
        ] {
            assert!(Game::new(bad, None).is_err(), "{bad}");
        }
        let g = Game::new("dice", None).expect("dice");
        assert_eq!(
            (g.type_name.as_str(), g.magic.as_str(), g.prefix.as_str()),
            ("Dice", "DICE", "dice1:")
        );
        assert_eq!(Game::new("go", None).expect("go").magic, "GOXX");
        assert_eq!(
            Game::new("blackjack", Some("BJ21")).expect("bj").magic,
            "BJ21"
        );
        assert!(Game::new("dice", Some("TOOLONG")).is_err());
    }

    #[test]
    fn rendering_leaves_none_of_the_coin_flips_names() {
        let files = render(&Game::new("dice", None).expect("dice"), "../../..").expect("render");
        for (path, text) in &files {
            // Identifiers only: the header says where the template came from.
            let lower = text.to_lowercase();
            assert!(
                !lower.contains("coinflip") && !text.contains("ZCFL") && !text.contains("Flip"),
                "{}",
                path.display()
            );
        }
        let tests = &files
            .iter()
            .find(|(p, _)| p.ends_with("dice.rs"))
            .expect("tests")
            .1;
        let kat = Game::new("dice", None)
            .expect("dice")
            .known_answers()
            .expect("kat");
        assert!(
            tests.contains(&kat),
            "the known answers are recomputed for zecraft/dice/v1"
        );
    }
}
