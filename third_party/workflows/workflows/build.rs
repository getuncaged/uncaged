use anyhow::Result;
use convert_case::{Case, Casing};
use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use walkdir::WalkDir;
use warp_workflows_types::Workflow;

/// Generates Workflows as Rust code from the yaml stored in /specs. Each Workflow is stored within
/// its own mod within the `generated_workflows` module. Additionally, a function called `workflows`
/// is generated that returns a vector of all the `Workflow`s that were created.
///
/// Uncaged: the generated code is written to a single file in `$OUT_DIR`, which `src/lib.rs`
/// `include!`s, instead of a module tree under `src/generated_workflows/` followed by `cargo fmt`.
fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=../specs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let parent_module = File::create(out_dir.join("generated_workflows.rs"))?;

    let mut workflows_added = Vec::new();

    for entry in WalkDir::new("../specs") {
        let entry = entry?;
        let valid_yaml_file = entry
            .path()
            .extension()
            .and_then(OsStr::to_str)
            .map_or(false, |extension| extension == "yaml" || extension == "yml");

        if valid_yaml_file {
            let file = File::open(entry.path())?;

            let mmap = unsafe { memmap::Mmap::map(&file) }?;
            let yaml_content = std::str::from_utf8(&mmap)?;

            println!("attempting to generate workflow at {:?}", entry.path());
            let workflow: Workflow = serde_yaml::from_str(yaml_content)?;
            println!("generated workflow is {workflow:?}");

            let file_name = entry
                .file_name()
                .to_str()
                .expect("OsStr should convert to str")
                .replace(".yaml", "")
                .replace(".yml", "")
                .to_case(Case::Snake);
            println!("file name is {file_name:?}");

            // Create a module for each Workflow within the parent module.
            workflows_added.push(file_name.clone());

            write_workflow(&parent_module, workflow, file_name.as_str())?;
        }
    }

    write_workflows_function(parent_module, &mut workflows_added)?;

    Ok(())
}

/// Writes a `workflows` function into the module at path `parent_modules`. The generated function
/// will look approximately like:
/// ```ignore
/// use warp_workflows_metadata::*;
/// pub fn workflows() -> Vec<Workflow> {
///     vec![
///         foo::workflow(),
///         bar::workflow(),
///         bazz:workflow(),
///     ]
/// }
/// ```
fn write_workflows_function(
    mut parent_module: File,
    workflows_added: &mut Vec<String>,
) -> Result<()> {
    writeln!(parent_module, "\nuse warp_workflows_types::*;")?;
    writeln!(parent_module, "pub fn workflows() -> Vec<Workflow> {{")?;
    writeln!(parent_module, "vec![")?;
    for workflows in workflows_added {
        writeln!(parent_module, "{workflows}::workflow(),")?;
    }
    writeln!(parent_module, "]")?;
    writeln!(parent_module, "}}")?;

    Ok(())
}

/// Writes a Workflow as an inline module named `file_name` into `parent_module`. The generated
/// Rust will look like:
/// ```ignore
/// pub mod <file_name> {
/// use warp_workflows_metadata::*;
/// pub fn workflow -> Workflow {
///    Workflow {
///      ...
///    }
/// }
/// }
/// ```
fn write_workflow(mut parent_module: &File, workflow: Workflow, file_name: &str) -> Result<()> {
    writeln!(parent_module, "pub mod {file_name} {{")?;
    writeln!(parent_module, "use warp_workflows_types::*;")?;
    writeln!(parent_module, r#"pub fn workflow() -> Workflow {{"#)?;
    writeln!(parent_module, "{}", uneval::to_string(workflow)?)?;
    writeln!(parent_module, "}}")?;
    writeln!(parent_module, "}}")?;

    Ok(())
}
