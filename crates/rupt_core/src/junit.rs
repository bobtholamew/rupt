use crate::results::{Outcome, SessionResult};
use std::io::Write;
use std::path::Path;

pub fn write_junit_xml(session: &SessionResult, path: &Path) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;

    let total = session.results.len();
    let failures = session.failed();
    let errors = session.errors();
    let skipped = session.skipped();
    let duration = session.total_duration;

    writeln!(f, r#"<?xml version="1.0" encoding="utf-8"?>"#)?;
    writeln!(
        f,
        r#"<testsuites><testsuite name="rupt" tests="{total}" failures="{failures}" errors="{errors}" skipped="{skipped}" time="{duration:.3}">"#
    )?;

    for result in &session.results {
        let (classname, name) = split_node_id(&result.node_id);
        let time = result.duration;

        write!(
            f,
            r#"<testcase classname="{}" name="{}" time="{time:.3}">"#,
            xml_escape(&classname),
            xml_escape(&name),
        )?;

        match result.outcome {
            Outcome::Failed => {
                writeln!(
                    f,
                    r#"<failure message="{}">{}</failure>"#,
                    xml_escape(&result.message),
                    xml_escape(&result.longrepr),
                )?;
            }
            Outcome::Error => {
                writeln!(
                    f,
                    r#"<error message="{}">{}</error>"#,
                    xml_escape(&result.message),
                    xml_escape(&result.longrepr),
                )?;
            }
            Outcome::Skipped | Outcome::Xfailed => {
                writeln!(
                    f,
                    r#"<skipped message="{}"/>"#,
                    xml_escape(&result.message),
                )?;
            }
            _ => {}
        }

        if !result.stdout.is_empty() {
            writeln!(f, "<system-out>{}</system-out>", xml_escape(&result.stdout))?;
        }
        if !result.stderr.is_empty() {
            writeln!(f, "<system-err>{}</system-err>", xml_escape(&result.stderr))?;
        }

        writeln!(f, "</testcase>")?;
    }

    writeln!(f, "</testsuite></testsuites>")?;
    Ok(())
}

fn split_node_id(node_id: &str) -> (String, String) {
    // "path/test_foo.py::TestClass::test_method" -> ("path.test_foo.TestClass", "test_method")
    let base = if let Some(idx) = node_id.find('[') {
        &node_id[..idx]
    } else {
        node_id
    };

    let parts: Vec<&str> = base.split("::").collect();
    match parts.len() {
        1 => (String::new(), parts[0].to_string()),
        2 => {
            let file = parts[0].replace('/', ".").replace('\\', ".").replace(".py", "");
            (file, parts[1].to_string())
        }
        _ => {
            let file = parts[0].replace('/', ".").replace('\\', ".").replace(".py", "");
            let classname = format!("{file}.{}", parts[1]);
            let name = parts[2..].join("::");
            (classname, name)
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
