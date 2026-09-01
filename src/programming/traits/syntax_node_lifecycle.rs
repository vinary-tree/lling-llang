use std::fmt;

use super::SyntaxNode;

impl Clone for SyntaxNode {
    fn clone(&self) -> Self {
        struct Frame<'a> {
            source: &'a SyntaxNode,
            next_child: usize,
            children: Vec<SyntaxNode>,
        }

        let mut frames = Vec::with_capacity(64);
        frames.push(Frame {
            source: self,
            next_child: 0,
            children: Vec::with_capacity(self.children.len()),
        });

        loop {
            let frame = frames
                .last_mut()
                .expect("the root syntax-node clone frame remains until completion");
            if let Some(child) = frame.source.children.get(frame.next_child) {
                frame.next_child += 1;
                frames.push(Frame {
                    source: child,
                    next_child: 0,
                    children: Vec::with_capacity(child.children.len()),
                });
                continue;
            }

            let completed = frames
                .pop()
                .expect("a completed syntax-node clone frame is present");
            let clone = SyntaxNode {
                kind: completed.source.kind.clone(),
                range: completed.source.range,
                text: completed.source.text.clone(),
                children: completed.children,
                is_error: completed.source.is_error,
                is_missing: completed.source.is_missing,
            };
            if let Some(parent) = frames.last_mut() {
                parent.children.push(clone);
            } else {
                return clone;
            }
        }
    }
}

impl PartialEq for SyntaxNode {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = Vec::with_capacity(64);
        pending.push((self, other));
        while let Some((left, right)) = pending.pop() {
            if std::ptr::eq(left, right) {
                continue;
            }
            if left.kind != right.kind
                || left.range != right.range
                || left.text != right.text
                || left.is_error != right.is_error
                || left.is_missing != right.is_missing
                || left.children.len() != right.children.len()
            {
                return false;
            }
            pending.extend(left.children.iter().zip(&right.children).rev());
        }
        true
    }
}

enum DebugEvent<'a> {
    CompactNode(&'a SyntaxNode),
    CompactSuffix(&'a SyntaxNode),
    PrettyNode(&'a SyntaxNode, usize),
    PrettySuffix(&'a SyntaxNode, usize),
    Indent(usize),
    Text(&'static str),
}

fn write_indent(formatter: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        formatter.write_str("    ")?;
    }
    Ok(())
}

fn write_pretty_kind(
    formatter: &mut fmt::Formatter<'_>,
    node: &SyntaxNode,
    field_depth: usize,
) -> fmt::Result {
    formatter.write_str("NodeKind(\n")?;
    write_indent(formatter, field_depth + 1)?;
    writeln!(formatter, "{:?},", node.kind.0)?;
    write_indent(formatter, field_depth)?;
    formatter.write_str(")")
}

fn write_pretty_position(
    formatter: &mut fmt::Formatter<'_>,
    position: super::Position,
    field_depth: usize,
) -> fmt::Result {
    formatter.write_str("Position {\n")?;
    write_indent(formatter, field_depth + 1)?;
    writeln!(formatter, "line: {},", position.line)?;
    write_indent(formatter, field_depth + 1)?;
    writeln!(formatter, "column: {},", position.column)?;
    write_indent(formatter, field_depth + 1)?;
    writeln!(formatter, "byte_offset: {},", position.byte_offset)?;
    write_indent(formatter, field_depth)?;
    formatter.write_str("}")
}

fn write_pretty_range(
    formatter: &mut fmt::Formatter<'_>,
    node: &SyntaxNode,
    field_depth: usize,
) -> fmt::Result {
    formatter.write_str("Range {\n")?;
    write_indent(formatter, field_depth + 1)?;
    formatter.write_str("start: ")?;
    write_pretty_position(formatter, node.range.start, field_depth + 1)?;
    formatter.write_str(",\n")?;
    write_indent(formatter, field_depth + 1)?;
    formatter.write_str("end: ")?;
    write_pretty_position(formatter, node.range.end, field_depth + 1)?;
    formatter.write_str(",\n")?;
    write_indent(formatter, field_depth)?;
    formatter.write_str("}")
}

fn write_pretty_text(
    formatter: &mut fmt::Formatter<'_>,
    node: &SyntaxNode,
    field_depth: usize,
) -> fmt::Result {
    if let Some(text) = &node.text {
        formatter.write_str("Some(\n")?;
        write_indent(formatter, field_depth + 1)?;
        writeln!(formatter, "{text:?},")?;
        write_indent(formatter, field_depth)?;
        formatter.write_str(")")
    } else {
        formatter.write_str("None")
    }
}

impl fmt::Debug for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut events = Vec::with_capacity(64);
        if formatter.alternate() {
            events.push(DebugEvent::PrettyNode(self, 0));
        } else {
            events.push(DebugEvent::CompactNode(self));
        }

        while let Some(event) = events.pop() {
            match event {
                DebugEvent::Text(text) => formatter.write_str(text)?,
                DebugEvent::Indent(depth) => write_indent(formatter, depth)?,
                DebugEvent::CompactNode(node) => {
                    write!(
                        formatter,
                        "SyntaxNode {{ kind: {:?}, range: {:?}, text: {:?}, children: [",
                        node.kind, node.range, node.text
                    )?;
                    events.push(DebugEvent::CompactSuffix(node));
                    for (index, child) in node.children.iter().enumerate().rev() {
                        if index + 1 < node.children.len() {
                            events.push(DebugEvent::Text(", "));
                        }
                        events.push(DebugEvent::CompactNode(child));
                    }
                }
                DebugEvent::CompactSuffix(node) => {
                    write!(
                        formatter,
                        "], is_error: {:?}, is_missing: {:?} }}",
                        node.is_error, node.is_missing
                    )?;
                }
                DebugEvent::PrettyNode(node, depth) => {
                    formatter.write_str("SyntaxNode {\n")?;
                    write_indent(formatter, depth + 1)?;
                    formatter.write_str("kind: ")?;
                    write_pretty_kind(formatter, node, depth + 1)?;
                    formatter.write_str(",\n")?;
                    write_indent(formatter, depth + 1)?;
                    formatter.write_str("range: ")?;
                    write_pretty_range(formatter, node, depth + 1)?;
                    formatter.write_str(",\n")?;
                    write_indent(formatter, depth + 1)?;
                    formatter.write_str("text: ")?;
                    write_pretty_text(formatter, node, depth + 1)?;
                    formatter.write_str(",\n")?;
                    write_indent(formatter, depth + 1)?;
                    if node.children.is_empty() {
                        formatter.write_str("children: [],\n")?;
                        write_indent(formatter, depth + 1)?;
                        writeln!(formatter, "is_error: {:?},", node.is_error)?;
                        write_indent(formatter, depth + 1)?;
                        writeln!(formatter, "is_missing: {:?},", node.is_missing)?;
                        write_indent(formatter, depth)?;
                        formatter.write_str("}")?;
                    } else {
                        formatter.write_str("children: [\n")?;
                        events.push(DebugEvent::PrettySuffix(node, depth));
                        for child in node.children.iter().rev() {
                            events.push(DebugEvent::Text(",\n"));
                            events.push(DebugEvent::PrettyNode(child, depth + 2));
                            events.push(DebugEvent::Indent(depth + 2));
                        }
                    }
                }
                DebugEvent::PrettySuffix(node, depth) => {
                    write_indent(formatter, depth + 1)?;
                    formatter.write_str("],\n")?;
                    write_indent(formatter, depth + 1)?;
                    writeln!(formatter, "is_error: {:?},", node.is_error)?;
                    write_indent(formatter, depth + 1)?;
                    writeln!(formatter, "is_missing: {:?},", node.is_missing)?;
                    write_indent(formatter, depth)?;
                    formatter.write_str("}")?;
                }
            }
        }
        Ok(())
    }
}

impl Drop for SyntaxNode {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut node) = pending.pop() {
            pending.extend(std::mem::take(&mut node.children));
        }
    }
}
