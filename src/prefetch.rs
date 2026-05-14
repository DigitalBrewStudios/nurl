use std::{
    io::BufRead,
    process::{Command, Output, Stdio},
};

use eyre::{Result, bail, eyre};
use nix_bindings_expr::eval_state::EvalState;
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::FlakeReferenceParseFlags;
use nix_bindings_store::store::Store;
use serde::Deserialize;

#[derive(Deserialize)]
struct PrefetchOutput {
    hash: String,
}

trait GetStdout {
    fn get_stdout(&mut self) -> Result<Vec<u8>>;
}

impl GetStdout for Command {
    fn get_stdout(&mut self) -> Result<Vec<u8>> {
        let Output { stdout, status, .. } = self.stderr(Stdio::inherit()).output()?;
        if !status.success() {
            bail!("command exited with {}", status);
        }
        Ok(stdout)
    }
}

macro_rules! info {
    ($($tt:tt)+) => {{
        use owo_colors::{OwoColorize, Stream, Style};
        eprintln!(
            "{}",
            format_args!($($tt)+).if_supports_color(Stream::Stderr, |text| text
                .style(Style::new().blue().bold()))
        );
    }};
}

pub struct Prefetch {
    eval_state: EvalState,
    fetch_settings: FetchersSettings,
}

impl Prefetch {
    pub fn new() -> Self {
        Self {
            eval_state: EvalState::new(Store::open(None, []).unwrap(), []).unwrap(),
            fetch_settings: FetchersSettings::new().unwrap(),
        }
    }

    pub fn flake_prefetch(&mut self, reference: &str) -> Result<String> {
        let flake_settings = nix_bindings_flake::FlakeSettings::new().unwrap();
        let flake_ref = nix_bindings_flake::FlakeReference::parse_with_fragment(
            &self.fetch_settings,
            &flake_settings,
            &FlakeReferenceParseFlags::new(&flake_settings).unwrap(),
            reference,
        )
        .unwrap();

        let locked_flake = nix_bindings_flake::LockedFlake::lock(
            &self.fetch_settings,
            &flake_settings,
            &self.eval_state,
            &nix_bindings_flake::FlakeLockFlags::new(&flake_settings).unwrap(),
            &flake_ref.0,
        )
        .unwrap();

        let outputs = locked_flake
            .outputs(&flake_settings, &mut self.eval_state)
            .unwrap();

        Ok("".to_string())
    }

    // work around for https://github.com/NixOS/nix/issues/5291
    pub fn git_prefetch(
        &mut self,
        git_scheme: bool,
        url: &str,
        rev: &str,
        submodules: bool,
    ) -> Result<String> {
        let prefix = if git_scheme { "" } else { "git+" };
        let submodules = if submodules { "&submodules=1" } else { "" };

        if rev.len() == 40 {
            self.flake_prefetch(format!("{prefix}{url}?allRefs=1&rev={rev}{submodules}").as_str())
        } else {
            if !rev.starts_with("refs/")
                && let hash @ Ok(_) = self.flake_prefetch(
                    format!("{prefix}{url}?ref=refs/tags/{rev}{submodules}").as_str(),
                )
            {
                return hash;
            }
            self.flake_prefetch(format!("{prefix}{url}?ref={rev}{submodules}").as_str())
        }
    }
}

// work around for https://github.com/NixOS/nix/issues/5291
// pub fn git_prefetch(git_scheme: bool, url: &str, rev: &str, submodules: bool) -> Result<String> {
//     let prefix = if git_scheme { "" } else { "git+" };
//     let submodules = if submodules { "&submodules=1" } else { "" };
//
//     if rev.len() == 40 {
//         flake_prefetch(format!("{prefix}{url}?allRefs=1&rev={rev}{submodules}").as_str())
//     } else {
//         if !rev.starts_with("refs/")
//             && let hash @ Ok(_) =
//                 flake_prefetch(format!("{prefix}{url}?ref=refs/tags/{rev}{submodules}").as_str())
//         {
//             return hash;
//         }
//         flake_prefetch(format!("{prefix}{url}?ref={rev}{submodules}").as_str())
//     }
// }

pub fn url_prefetch(url: &str) -> Result<String> {
    info!("$ nix store prefetch-file --json {url}");

    Ok(serde_json::from_slice::<PrefetchOutput>(
        &Command::new("nix")
            .arg("store")
            .arg("prefetch-file")
            .arg("--extra-experimental-features")
            .arg("nix-command")
            .arg("--json")
            .arg(url)
            .get_stdout()?,
    )?
    .hash)
}

pub fn fod_prefetch(expr: String) -> Result<String> {
    info!(
        "$ nix build --extra-experimental-features 'nix-command flakes' --impure --no-link --expr '{expr}'"
    );

    let Output {
        stdout,
        stderr,
        status,
    } = Command::new("nix")
        .arg("build")
        .arg("--extra-experimental-features")
        .arg("nix-command flakes")
        .arg("--impure")
        .arg("--no-link")
        .arg("--expr")
        .arg(expr)
        .output()?;

    if status.success() {
        bail!(
            "command succeeded unexpectedly\nstdout:\n{}",
            String::from_utf8_lossy(&stdout),
        );
    }

    let mut lines = stderr.lines();
    while let Some(line) = lines.next() {
        if !matches!(line, Ok(line) if line.trim_start().starts_with("specified:")) {
            continue;
        }
        let Some(line) = lines.next() else {
            break;
        };
        if let Ok(line) = line {
            let Some(hash) = line.trim_start().strip_prefix("got:") else {
                continue;
            };
            return Ok(hash.trim().into());
        }
    }

    Err(eyre!(
        "failed to find the hash from error messages\nstdout: {}\nstderr:\n{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr),
    ))
}
