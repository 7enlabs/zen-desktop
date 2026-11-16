use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointInfo {
    pub id: String,

    pub message: String,

    pub timestamp: i64,
}

#[derive(Clone, Debug)]
pub struct RestoreReport {
    pub restored_files: Vec<String>,

    pub new_checkpoint_id: String,
}

pub struct CheckpointManager {
    workdir: PathBuf,
    repo: PathBuf,
}

impl CheckpointManager {
    pub fn new(workdir: impl AsRef<Path>) -> Result<Self> {
        let workdir = workdir.as_ref().to_path_buf();
        let repo = workdir.join(".zen").join("checkpoints");
        fs::create_dir_all(&repo)
            .with_context(|| format!("create shadow repo dir {}", repo.display()))?;
        if !repo.join(".git").exists() {
            git(&repo, &["init"]).context("git init shadow repo")?;
        }

        git(&repo, &["config", "user.name", "zen-checkpoints"]).context("git config user.name")?;
        git(&repo, &["config", "user.email", "checkpoints@zen.local"])
            .context("git config user.email")?;
        git(&repo, &["config", "commit.gpgsign", "false"]).context("git config commit.gpgsign")?;

        git(&repo, &["config", "core.autocrlf", "false"]).context("git config core.autocrlf")?;
        Ok(Self { workdir, repo })
    }

    pub fn snapshot(&self, message: &str) -> Result<String> {
        if message.contains('\n') {
            bail!("snapshot message must be single-line");
        }
        self.sync_workdir_to_repo()?;
        git(&self.repo, &["add", "-A"]).context("git add")?;
        let no_changes = self.head_exists()?
            && git(&self.repo, &["status", "--porcelain"])?
                .trim()
                .is_empty();
        if no_changes {
            return self.short_head();
        }
        let mut args = vec!["commit", "-m", message];
        if !self.head_exists()? {
            args.push("--allow-empty");
        }
        git(&self.repo, &args).with_context(|| format!("git commit: {message}"))?;
        self.short_head()
    }

    pub fn list(&self) -> Result<Vec<CheckpointInfo>> {
        if !self.head_exists()? {
            return Ok(vec![]);
        }
        let out = git(&self.repo, &["log", "--pretty=format:%h\x1f%ct\x1f%s"])?;
        let mut infos = Vec::new();
        for line in out.lines() {
            let mut parts = line.splitn(3, '\x1f');
            let (Some(id), Some(ts), Some(message)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            infos.push(CheckpointInfo {
                id: id.to_string(),
                message: message.to_string(),
                timestamp: ts.parse().unwrap_or(0),
            });
        }
        Ok(infos)
    }

    pub fn restore(&self, id: &str) -> Result<RestoreReport> {
        let full = git(
            &self.repo,
            &["rev-parse", "--verify", &format!("{id}^{{commit}}")],
        )
        .with_context(|| format!("unknown checkpoint id: {id}"))?;
        let full = full.trim().to_string();
        let new_checkpoint_id = self.snapshot(&format!("auto: before restore to {id}"))?;
        let tree = git_bytes(&self.repo, &["ls-tree", "-r", "--name-only", "-z", &full])
            .context("git ls-tree")?;
        let mut restored_files = Vec::new();
        for raw in tree.split(|b| *b == 0) {
            if raw.is_empty() {
                continue;
            }
            let rel = String::from_utf8_lossy(raw).into_owned();
            let content = git_bytes(&self.repo, &["show", &format!("{full}:{rel}")])
                .with_context(|| format!("git show {rel}"))?;
            let dest = self.workdir.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create parent dir for {rel}"))?;
            }
            fs::write(&dest, content).with_context(|| format!("write {rel}"))?;
            restored_files.push(rel);
        }
        Ok(RestoreReport {
            restored_files,
            new_checkpoint_id,
        })
    }

    fn sync_workdir_to_repo(&self) -> Result<()> {
        for entry in fs::read_dir(&self.repo).context("read shadow repo")? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let p = entry.path();
            if entry
                .file_type()
                .with_context(|| format!("file type of {}", p.display()))?
                .is_dir()
            {
                fs::remove_dir_all(&p)?;
            } else {
                fs::remove_file(&p)?;
            }
        }
        let mut stack = vec![self.workdir.clone()];
        while let Some(dir) = stack.pop() {
            for entry in
                fs::read_dir(&dir).with_context(|| format!("read dir {}", dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let rel = path
                    .strip_prefix(&self.workdir)
                    .with_context(|| format!("relativize {}", path.display()))?;
                if rel.starts_with(".zen") || rel.starts_with(".git") {
                    continue;
                }
                let ft = entry
                    .file_type()
                    .with_context(|| format!("file type of {}", path.display()))?;
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    stack.push(path);
                    continue;
                }
                let dest = self.repo.join(rel);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&path, &dest)
                    .with_context(|| format!("copy {} to shadow repo", rel.display()))?;
            }
        }
        Ok(())
    }

    fn head_exists(&self) -> Result<bool> {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(&self.repo)
            .output()
            .context("git rev-parse")?;
        Ok(out.status.success())
    }

    fn short_head(&self) -> Result<String> {
        Ok(git(&self.repo, &["rev-parse", "--short", "HEAD"])?
            .trim()
            .to_string())
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmpdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zen-ckpt-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).expect("create test tempdir");
        dir
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let wd = tmpdir();
        let mgr = CheckpointManager::new(&wd).expect("new manager");
        fs::write(wd.join("a.txt"), "v1").unwrap();
        fs::create_dir_all(wd.join("sub")).unwrap();
        fs::write(wd.join("sub").join("b.txt"), "hello").unwrap();

        let id1 = mgr.snapshot("first").expect("snapshot 1");
        assert!(!id1.is_empty());

        let id1b = mgr.snapshot("first again").expect("snapshot 1b");
        assert_eq!(id1, id1b);
        assert_eq!(mgr.list().expect("list").len(), 1);

        fs::write(wd.join("a.txt"), "v2").unwrap();
        fs::write(wd.join("new.txt"), "brand new").unwrap();
        fs::remove_file(wd.join("sub").join("b.txt")).unwrap();
        let id2 = mgr.snapshot("second").expect("snapshot 2");
        assert_ne!(id1, id2);

        let list = mgr.list().expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2);
        assert_eq!(list[1].id, id1);
        assert_eq!(list[0].message, "second");
        assert!(list[0].timestamp >= list[1].timestamp);

        fs::write(wd.join("a.txt"), "v3").unwrap();

        let report = mgr.restore(&id1).expect("restore");
        assert_eq!(fs::read_to_string(wd.join("a.txt")).unwrap(), "v1");
        assert_eq!(
            fs::read_to_string(wd.join("sub").join("b.txt")).unwrap(),
            "hello"
        );

        assert_eq!(fs::read_to_string(wd.join("new.txt")).unwrap(), "brand new");
        assert!(report.restored_files.contains(&"a.txt".to_string()));
        assert!(!report.restored_files.iter().any(|f| f.starts_with(".zen")));

        let list2 = mgr.list().expect("list after restore");
        assert_eq!(list2.len(), 3);
        assert_eq!(list2[0].id, report.new_checkpoint_id);
        assert!(list2[0].message.contains("before restore"));

        mgr.restore(&report.new_checkpoint_id)
            .expect("undo restore");
        assert_eq!(fs::read_to_string(wd.join("a.txt")).unwrap(), "v3");
    }

    #[test]
    fn restore_unknown_id_fails() {
        let wd = tmpdir();
        let mgr = CheckpointManager::new(&wd).expect("new manager");
        fs::write(wd.join("a.txt"), "x").unwrap();
        mgr.snapshot("s").expect("snapshot");
        assert!(mgr.restore("deadbee").is_err());
    }

    #[test]
    fn fresh_manager_has_empty_list() {
        let wd = tmpdir();
        let mgr = CheckpointManager::new(&wd).expect("new manager");
        assert!(mgr.list().expect("list").is_empty());
    }

    #[test]
    fn snapshot_rejects_multiline_message() {
        let wd = tmpdir();
        let mgr = CheckpointManager::new(&wd).expect("new manager");
        assert!(mgr.snapshot("line1\nline2").is_err());
    }
}
