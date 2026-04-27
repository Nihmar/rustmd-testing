use chrono::Local;
use markdown::mdast;
use markdown::to_mdast;
use pulldown_cmark::{html, Options, Parser};
use regex::{Captures, Regex};
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

/// Render markdown using pulldown-cmark.
///
/// Enables math (inline `$...$` and display `$$...$$`) and wikilinks
/// (`[[link]]` and `[[link|text]]`) support via extension flags.
fn render_with_pulldown_cmark(markdown: &str) -> String {
    // Enable math, wikilinks, tables, strikethrough, and other useful extensions
    let mut options = Options::empty();
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_WIKILINKS);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Render markdown using comrak.
///
/// Enables math (`$...$` inline, `$$...$$` display) and wikilinks
/// (`[[link]]` and `[[link|text]]`) via extension options.
fn render_with_comrak(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    // Enable dollar-sign math: $inline$ and $$display$$
    options.extension.math_dollars = true;
    // Enable wikilinks with URL-first syntax: [[url|text]]
    options.extension.wikilinks_title_after_pipe = true;
    comrak::markdown_to_html(markdown, &options)
}

/// Preprocess markdown to convert wikilinks into standard markdown links.
///
/// Handles two Obsidian-style wikilink patterns:
/// - `[[target]]` → `[target](target)` (plain reference)
/// - `[[target|text]]` → `[text](target)` (with display text)
///
/// Note: Rust's `regex` crate does not support look-around assertions,
/// so we use a simpler pattern that matches all `[[...]]` occurrences.
fn preprocess_wikilinks(markdown: &str) -> String {
    // Match [[target|text]] or [[target]]
    // Capture group 1: the target (characters up to | or ])
    // Capture group 2 (optional): the display text after pipe (characters up to ])
    // Using ([^]|]+) for group 1 ensures the pipe is captured as the separator
    let re = Regex::new(r"\[\[([^]|]+)(?:\|([^\]]*))?\]\]")
        .expect("Failed to compile wikilink regex");

    re.replace_all(markdown, |caps: &Captures| {
        let target = &caps[1];
        let text = caps.get(2).map(|m| m.as_str()).unwrap_or(target);
        format!("[{}]({})", text, target)
    })
    .to_string()
}

/// Render markdown using markdown (mdast) crate by serializing its AST to HTML.
///
/// Preprocesses wikilinks into standard markdown links before parsing, since the
/// mdast specification has no native wikilink node type. Enables math parsing
/// via `math_flow` and `math_text` constructs.
fn render_with_markdown_crate(markdown: &str) -> String {
    // Preprocess wikilinks before parsing so the mdast has no wikilink nodes
    let processed = preprocess_wikilinks(markdown);
    let ast = to_mdast(
        &processed,
        &markdown::ParseOptions {
            constructs: markdown::Constructs {
                math_flow: true,
                math_text: true,
                ..markdown::Constructs::default()
            },
            ..markdown::ParseOptions::default()
        },
    )
    .expect("Failed to parse markdown to mdast");
    serialize_mdast_to_html(&ast)
}

/// Recursively serialize mdast nodes to HTML
fn serialize_mdast_to_html(node: &mdast::Node) -> String {
    let mut html_output = String::new();
    serialize_mdast_node(&mut html_output, node);
    html_output
}

fn serialize_mdast_node(html_output: &mut String, node: &mdast::Node) {
    match node {
        mdast::Node::Root(root) => {
            for child in &root.children {
                serialize_mdast_node(html_output, child);
            }
        }
        mdast::Node::Heading(heading) => {
            let depth = heading.depth;
            let inner: String = heading
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!("<h{depth}>{inner}</h{depth}>\n"));
        }
        mdast::Node::Paragraph(paragraph) => {
            let inner: String = paragraph
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!("<p>{inner}</p>\n"));
        }
        mdast::Node::Text(text) => {
            html_output.push_str(&text.value);
        }
        mdast::Node::Code(code) => {
            html_output.push_str(&format!("<pre><code>{}</code></pre>\n", code.value));
        }
        mdast::Node::ThematicBreak(_) => {
            html_output.push_str("<hr />\n");
        }
        mdast::Node::Blockquote(blockquote) => {
            html_output.push_str("<blockquote>\n");
            for child in &blockquote.children {
                serialize_mdast_node(html_output, child);
            }
            html_output.push_str("</blockquote>\n");
        }
        mdast::Node::List(list) => {
            let tag = if list.ordered { "ol" } else { "ul" };
            html_output.push_str(&format!("<{tag}>\n"));
            for item in &list.children {
                if let mdast::Node::ListItem(list_item) = item {
                    for child in &list_item.children {
                        serialize_mdast_node(html_output, child);
                    }
                }
            }
            html_output.push_str(&format!("</{tag}>\n"));
        }
        mdast::Node::ListItem(list_item) => {
            html_output.push_str("<li>");
            for child in &list_item.children {
                serialize_mdast_node(html_output, child);
            }
            html_output.push_str("</li>\n");
        }
        mdast::Node::Html(html_node) => {
            html_output.push_str(&html_node.value);
        }
        mdast::Node::Break(_) => {
            html_output.push_str("<br />\n");
        }
        mdast::Node::InlineCode(inline_code) => {
            html_output.push_str(&format!("<code>{}</code>", inline_code.value));
        }
        mdast::Node::Emphasis(emphasis) => {
            let inner: String = emphasis
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!("<em>{inner}</em>"));
        }
        mdast::Node::Strong(strong) => {
            let inner: String = strong
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!("<strong>{inner}</strong>"));
        }
        mdast::Node::Link(link) => {
            let inner: String = link
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                link.url, inner
            ));
        }
        mdast::Node::Image(image) => {
            let alt = image.title.as_deref().unwrap_or("");
            html_output.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" />",
                image.url, alt
            ));
        }
        mdast::Node::Table(table) => {
            html_output.push_str("<table>\n");
            for child in &table.children {
                serialize_mdast_node(html_output, child);
            }
            html_output.push_str("</table>\n");
        }
        mdast::Node::TableRow(table_row) => {
            html_output.push_str("<tr>\n");
            for child in &table_row.children {
                serialize_mdast_node(html_output, child);
            }
            html_output.push_str("</tr>\n");
        }
        mdast::Node::TableCell(table_cell) => {
            let inner: String = table_cell
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!("<td>{inner}</td>\n"));
        }
        mdast::Node::Math(math) => {
            // Display math block: wrap in a styled pre/code block for proper rendering
            html_output.push_str(&format!(
                "<pre><code class=\"math math-display\">{}</code></pre>\n",
                math.value
            ));
        }
        mdast::Node::InlineMath(inline_math) => {
            // Inline math: wrap in a span for proper rendering alongside text
            html_output.push_str(&format!(
                "<span class=\"math math-inline\">{}</span>",
                inline_math.value
            ));
        }
        mdast::Node::Delete(delete) => {
            let inner: String = delete
                .children
                .iter()
                .map(serialize_mdast_str)
                .collect();
            html_output.push_str(&format!("<del>{inner}</del>"));
        }
        // Handle remaining node types gracefully (FootnoteDefinition, Definition, etc.)
        _ => {}
    }
}

fn serialize_mdast_str(node: &mdast::Node) -> String {
    let mut result = String::new();
    serialize_mdast_node(&mut result, node);
    result
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <markdown-file.md>", args[0]);
        std::process::exit(1);
    }

    let md_path = &args[1];
    let path = Path::new(md_path);

    if !path.exists() {
        eprintln!("Error: File '{}' does not exist.", md_path);
        std::process::exit(1);
    }

    let markdown = fs::read_to_string(md_path).expect("Failed to read markdown file");
    let file_stem = path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Create "rendered" subfolder in the same directory as the markdown file
    let rendered_dir = path.parent().unwrap().join("rendered");
    fs::create_dir_all(&rendered_dir).expect("Failed to create rendered directory");

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");

    // --- pulldown-cmark ---
    let start = Instant::now();
    let html_pulldown = render_with_pulldown_cmark(&markdown);
    let elapsed_pulldown = start.elapsed();
    let file_pulldown = rendered_dir.join(format!("{}.pulldown-cmark.html", file_stem));
    let mut f = fs::File::create(&file_pulldown).expect("Failed to create file");
    f.write_all(html_pulldown.as_bytes()).unwrap();

    // --- comrak ---
    let start = Instant::now();
    let html_comrak = render_with_comrak(&markdown);
    let elapsed_comrak = start.elapsed();
    let file_comrak = rendered_dir.join(format!("{}.comrak.html", file_stem));
    let mut f = fs::File::create(&file_comrak).expect("Failed to create file");
    f.write_all(html_comrak.as_bytes()).unwrap();

    // --- markdown (mdast) crate ---
    let start = Instant::now();
    let html_markdown = render_with_markdown_crate(&markdown);
    let elapsed_markdown = start.elapsed();
    let file_markdown = rendered_dir.join(format!("{}.markdown.html", file_stem));
    let mut f = fs::File::create(&file_markdown).expect("Failed to create file");
    f.write_all(html_markdown.as_bytes()).unwrap();

    // Print timings
    println!("=== Markdown Rendering Timings ===");
    println!("Source: {}", md_path);
    println!("Output directory: {}", rendered_dir.display());
    println!("Timestamp: {}", timestamp);
    println!();
    println!(
        "  pulldown-cmark: {:.6} ms  ->  {}",
        elapsed_pulldown.as_secs_f64() * 1000.0,
        file_pulldown.display()
    );
    println!(
        "  comrak:         {:.6} ms  ->  {}",
        elapsed_comrak.as_secs_f64() * 1000.0,
        file_comrak.display()
    );
    println!(
        "  markdown:       {:.6} ms  ->  {}",
        elapsed_markdown.as_secs_f64() * 1000.0,
        file_markdown.display()
    );
    println!();
    println!("=== Done ===");
}
