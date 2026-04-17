use crate::services::remote::FileOps;
use anyhow::Result;
use directories::BaseDirs;
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Memory scope determines which CLAUDE.md file to read/write
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    User,
    Project,
    Local,
}

/// Information about a single CLAUDE.md file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFileInfo {
    pub scope: String,
    pub exists: bool,
    pub file_path: String,
    pub content: String,
    pub last_modified: Option<String>,
    pub size_bytes: Option<u64>,
}

/// All memory files across all three scopes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllMemoryFiles {
    pub user: MemoryFileInfo,
    pub project: Option<MemoryFileInfo>,
    pub local: Option<MemoryFileInfo>,
}

/// Resolve the CLAUDE.md file path for a given scope
pub fn resolve_memory_path(scope: &MemoryScope, project_path: Option<&Path>, file_ops: &dyn FileOps) -> Result<PathBuf> {
    match scope {
        MemoryScope::User => {
            let base_dirs =
                BaseDirs::new().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
            Ok(base_dirs.home_dir().join(".claude").join("CLAUDE.md"))
        }
        MemoryScope::Project => {
            let project = project_path
                .ok_or_else(|| anyhow::anyhow!("Project path required for project scope"))?;
            let dotclaude_path = project.join(".claude").join("CLAUDE.md");
            let root_path = project.join("CLAUDE.md");
            if file_ops.exists(&dotclaude_path) {
                Ok(dotclaude_path)
            } else if file_ops.exists(&root_path) {
                Ok(root_path)
            } else {
                Ok(root_path)
            }
        }
        MemoryScope::Local => {
            let project = project_path
                .ok_or_else(|| anyhow::anyhow!("Project path required for local scope"))?;
            Ok(project.join("CLAUDE.local.md"))
        }
    }
}

/// Detect which project memory location variant is in use
/// Returns (path, variant) where variant is "root" or ".claude"
pub fn detect_project_memory_location(project_path: &Path, file_ops: &dyn FileOps) -> Result<(PathBuf, String)> {
    let dotclaude_path = project_path.join(".claude").join("CLAUDE.md");
    let root_path = project_path.join("CLAUDE.md");

    if file_ops.exists(&dotclaude_path) {
        Ok((dotclaude_path, ".claude".to_string()))
    } else if file_ops.exists(&root_path) {
        Ok((root_path, "root".to_string()))
    } else {
        Ok((root_path, "root".to_string()))
    }
}

/// Read a single memory file and return its info
pub fn read_memory_file(
    scope: &MemoryScope,
    project_path: Option<&Path>,
    file_ops: &dyn FileOps,
) -> Result<MemoryFileInfo> {
    let scope_str = match scope {
        MemoryScope::User => "user",
        MemoryScope::Project => "project",
        MemoryScope::Local => "local",
    };

    let path = resolve_memory_path(scope, project_path, file_ops)?;
    let path_str = path.to_string_lossy().to_string();

    if file_ops.exists(&path) {
        let content = file_ops.read_string(&path)?;
        let content = content.replace("\r\n", "\n");
        let (size_bytes, last_modified) = file_ops.metadata(&path);

        Ok(MemoryFileInfo {
            scope: scope_str.to_string(),
            exists: true,
            file_path: path_str,
            content,
            last_modified,
            size_bytes,
        })
    } else {
        Ok(MemoryFileInfo {
            scope: scope_str.to_string(),
            exists: false,
            file_path: path_str,
            content: String::new(),
            last_modified: None,
            size_bytes: None,
        })
    }
}

/// Read all memory files across all three scopes
pub fn read_all_memory_files(project_path: Option<&Path>, file_ops: &dyn FileOps) -> Result<AllMemoryFiles> {
    let user = read_memory_file(&MemoryScope::User, None, file_ops)?;

    let (project, local) = if let Some(pp) = project_path {
        let project_info = read_memory_file(&MemoryScope::Project, Some(pp), file_ops)?;
        let local_info = read_memory_file(&MemoryScope::Local, Some(pp), file_ops)?;
        (Some(project_info), Some(local_info))
    } else {
        (None, None)
    };

    Ok(AllMemoryFiles {
        user,
        project,
        local,
    })
}

/// Write content to a memory file, creating directories if needed
pub fn write_memory_file(
    scope: &MemoryScope,
    project_path: Option<&Path>,
    content: &str,
    file_ops: &dyn FileOps,
) -> Result<MemoryFileInfo> {
    let path = resolve_memory_path(scope, project_path, file_ops)?;

    if let Some(parent) = path.parent() {
        file_ops.create_dir_all(parent)?;
    }

    file_ops.backup(&path)?;
    file_ops.write_string(&path, content)?;

    read_memory_file(scope, project_path, file_ops)
}

/// Delete a memory file. No error if the file doesn't exist.
pub fn delete_memory_file(scope: &MemoryScope, project_path: Option<&Path>, file_ops: &dyn FileOps) -> Result<()> {
    let path = resolve_memory_path(scope, project_path, file_ops)?;
    if file_ops.exists(&path) {
        file_ops.remove_file(&path)?;
    }
    Ok(())
}

/// Render markdown content to HTML using pulldown-cmark
pub fn render_markdown(content: &str) -> Result<String> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    Ok(html_output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::remote::LocalFileOps;

    #[test]
    fn test_read_nonexistent_memory_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let info = read_memory_file(&MemoryScope::Project, Some(path), &LocalFileOps).unwrap();
        assert!(!info.exists);
        assert!(info.content.is_empty());
        assert_eq!(info.scope, "project");
    }

    #[test]
    fn test_write_and_read_memory_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let content = "# My Project\n\nSome instructions here.";
        let info = write_memory_file(&MemoryScope::Project, Some(path), content, &LocalFileOps).unwrap();

        assert!(info.exists);
        assert_eq!(info.content, content);
        assert!(info.size_bytes.unwrap() > 0);
        assert!(info.last_modified.is_some());
    }

    #[test]
    fn test_write_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Writing to user scope would create ~/.claude/ but we test local scope
        let content = "# Local overrides";
        let info = write_memory_file(&MemoryScope::Local, Some(path), content, &LocalFileOps).unwrap();
        assert!(info.exists);
        assert_eq!(info.content, content);
    }

    #[test]
    fn test_delete_memory_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Create then delete
        write_memory_file(&MemoryScope::Local, Some(path), "temp content", &LocalFileOps).unwrap();
        delete_memory_file(&MemoryScope::Local, Some(path), &LocalFileOps).unwrap();

        let info = read_memory_file(&MemoryScope::Local, Some(path), &LocalFileOps).unwrap();
        assert!(!info.exists);
    }

    #[test]
    fn test_delete_nonexistent_file_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Should not error
        delete_memory_file(&MemoryScope::Local, Some(path), &LocalFileOps).unwrap();
    }

    #[test]
    fn test_project_scope_prefers_dotclaude() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Create both files
        std::fs::write(path.join("CLAUDE.md"), "root content").unwrap();
        let dotclaude = path.join(".claude");
        std::fs::create_dir_all(&dotclaude).unwrap();
        std::fs::write(dotclaude.join("CLAUDE.md"), "dotclaude content").unwrap();

        let info = read_memory_file(&MemoryScope::Project, Some(path), &LocalFileOps).unwrap();
        assert!(info.exists);
        assert_eq!(info.content, "dotclaude content");
    }

    #[test]
    fn test_project_scope_falls_back_to_root() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        std::fs::write(path.join("CLAUDE.md"), "root content").unwrap();

        let info = read_memory_file(&MemoryScope::Project, Some(path), &LocalFileOps).unwrap();
        assert!(info.exists);
        assert_eq!(info.content, "root content");
    }

    #[test]
    fn test_detect_project_memory_location() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Neither exists
        let (_, variant) = detect_project_memory_location(path, &LocalFileOps).unwrap();
        assert_eq!(variant, "root");

        // Create root file
        std::fs::write(path.join("CLAUDE.md"), "root").unwrap();
        let (_, variant) = detect_project_memory_location(path, &LocalFileOps).unwrap();
        assert_eq!(variant, "root");

        // Create .claude file (should take priority)
        let dotclaude = path.join(".claude");
        std::fs::create_dir_all(&dotclaude).unwrap();
        std::fs::write(dotclaude.join("CLAUDE.md"), "dotclaude").unwrap();
        let (_, variant) = detect_project_memory_location(path, &LocalFileOps).unwrap();
        assert_eq!(variant, ".claude");
    }

    #[test]
    fn test_render_markdown() {
        let content = "# Hello\n\n- item 1\n- item 2\n\n**bold** text";
        let html = render_markdown(content).unwrap();
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<li>item 1</li>"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn test_normalize_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Write content with \r\n
        let file_path = path.join("CLAUDE.md");
        std::fs::write(&file_path, "line1\r\nline2\r\nline3").unwrap();

        let info = read_memory_file(&MemoryScope::Project, Some(path), &LocalFileOps).unwrap();
        assert_eq!(info.content, "line1\nline2\nline3");
    }

    #[test]
    fn test_empty_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        std::fs::write(path.join("CLAUDE.md"), "").unwrap();

        let info = read_memory_file(&MemoryScope::Project, Some(path), &LocalFileOps).unwrap();
        assert!(info.exists);
        assert!(info.content.is_empty());
        assert_eq!(info.size_bytes, Some(0));
    }

    #[test]
    fn test_read_all_memory_files_no_project() {
        // Without project path, only user is returned, project/local are None
        let all = read_all_memory_files(None, &LocalFileOps);
        assert!(all.is_ok());
        let all = all.unwrap();
        assert!(all.project.is_none());
        assert!(all.local.is_none());
    }

    #[test]
    fn test_read_all_memory_files_with_project() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let all = read_all_memory_files(Some(path), &LocalFileOps).unwrap();
        assert!(all.project.is_some());
        assert!(all.local.is_some());
        // Project and local don't exist yet
        assert!(!all.project.unwrap().exists);
        assert!(!all.local.unwrap().exists);
    }
}
