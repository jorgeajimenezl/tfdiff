//! Compact output for terminals and CI logs.
//!
//! Sigils instead of words, so CI secret masking cannot mangle it:
//!
//! ```text
//! resources   +  created      -  destroyed    -/+ recreated   ~  updated
//!             x  forgotten    => moved        ==  untouched (summary line only)
//! attributes  +  added        -  removed      ~   changed     ^v reordered
//!             ?  known only after apply       !   forces the recreation
//!             <  previous address
//! ```

use std::fmt::Write as _;
use std::io;

use super::Renderer;
use crate::labels;
use crate::redact::Node;
use crate::report::{Action, AttrChange, Class, Op, Report, ResourceReport};

/// Longest value printed before it is cut short.
const MAX_VALUE_CHARS: usize = 120;
const INDENT: &str = "    ";

pub struct Text;

impl Renderer for Text {
    fn name(&self) -> &'static str {
        "text"
    }

    fn render(&self, report: &Report, out: &mut dyn io::Write) -> io::Result<()> {
        let totals = report.totals;
        writeln!(
            out,
            "tfdiff: +{} -{} -/+{} ~{} x{} =>{} =={}",
            totals.provision,
            totals.teardown,
            totals.respawn,
            totals.mutate,
            totals.detach,
            totals.rehome,
            totals.steady,
        )?;
        for resource in &report.resources {
            write_resource(resource, out)?;
        }
        if let Some(gate) = &report.gate {
            write!(out, "[{}] {}", gate.expect.name(), gate.verdict.label())?;
            if !gate.offenders.is_empty() {
                write!(out, ": {}", gate.offenders.join(", "))?;
            }
            writeln!(out)?;
        }
        Ok(())
    }
}

fn write_resource(resource: &ResourceReport, out: &mut dyn io::Write) -> io::Result<()> {
    let sigil = match resource.action {
        Action::Provision => "+",
        Action::Teardown => "-",
        Action::Respawn => "-/+",
        Action::Detach => "x",
        Action::Steady if resource.previous_address.is_some() => "=>",
        _ => "~",
    };
    writeln!(out, "{sigil} {}", resource.address)?;
    if let Some(previous) = &resource.previous_address {
        writeln!(out, "{INDENT}< {previous}")?;
    }
    for path in &resource.forced_by {
        writeln!(out, "{INDENT}! {path}")?;
    }
    for change in &resource.changes {
        writeln!(out, "{INDENT}{}", change_line(change))?;
    }
    Ok(())
}

fn change_line(change: &AttrChange) -> String {
    let path = &change.path;
    if change.class == Class::Undetermined {
        return format!("? {path}");
    }
    let before = change.before.as_ref().map(|shown| value(shown.node()));
    let after = change.after.as_ref().map(|shown| value(shown.node()));
    match (change.op, before, after) {
        (Op::Shuffled, ..) => format!("^v {path}"),
        (Op::Gained, _, Some(after)) => format!("+ {path}: {after}"),
        (Op::Dropped, Some(before), _) => format!("- {path}: {before}"),
        (_, Some(before), Some(after)) => format!("~ {path}: {before} -> {after}"),
        _ => format!("~ {path}"),
    }
}

fn value(node: &Node) -> String {
    let mut text = String::new();
    write_node(node, &mut text);
    if text.chars().count() > MAX_VALUE_CHARS {
        text = text.chars().take(MAX_VALUE_CHARS).collect();
        text.push_str("...");
    }
    text
}

fn write_node(node: &Node, out: &mut String) {
    // Writing to a String cannot fail.
    let _ = match node {
        Node::Null => write!(out, "null"),
        Node::Bool(flag) => write!(out, "{flag}"),
        Node::Number(number) => write!(out, "{number}"),
        // Debug formatting quotes the string and escapes control characters,
        // so a value cannot inject terminal escapes or fake extra lines.
        Node::String(text) => write!(out, "{text:?}"),
        Node::Masked => write!(out, "({})", labels::MASKED),
        Node::MaskedItems(count) => write!(out, "({}: {count})", labels::MASKED),
        Node::Array(items) => {
            out.push('[');
            for (position, item) in items.iter().enumerate() {
                if position > 0 {
                    out.push_str(", ");
                }
                write_node(item, out);
            }
            out.push(']');
            Ok(())
        }
        Node::Object(fields) => {
            out.push('{');
            for (position, (key, item)) in fields.iter().enumerate() {
                if position > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{key}=");
                write_node(item, out);
            }
            out.push('}');
            Ok(())
        }
    };
}
