use std::env;
use std::io::{Write};
use std::fs::{self, File};
use futures::executor;
use orgize::{Org, export::{Container, Event, HtmlExport, TraversalContext, Traverser}, SyntaxKind, rowan::ast::AstNode};

// Seems odd that the generate script for AST is in javascript?
use sqlx::sqlite::SqlitePool;

use clap::Parser;

/// A program to convert org-roam documents into subtext.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// File path to the org file to convert
    #[arg(short, long)]
    filename: String,

    /// File path where you want the output written.
    #[arg(short, long)]
    output: String,

    #[arg(long)]
    debug: bool
}


pub struct SubtextExporter {
    sqlite: SqlitePool,
    // This shouldn't require HTML specific things.
    pub exporter: HtmlExport,
}

// DB values somehow come back extra quoted, so provide a utility to strip it
fn unquote(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}


impl SubtextExporter {
    fn new(pool: SqlitePool, exporter: HtmlExport) -> SubtextExporter {
        SubtextExporter {
            sqlite: pool,
            exporter,
            // find_title_by_id: &conn.prepare("select title from notes where id = '\":id\"' limit 1;").unwrap(),
        }
    }
}

impl AsMut<HtmlExport> for SubtextExporter {
    fn as_mut(&mut self) -> &mut HtmlExport {
        &mut self.exporter
    }
}

impl Traverser for SubtextExporter {
    fn event(&mut self, event: Event, ctx: &mut TraversalContext) {
        match event {
            Event::Text(value) => {
                let value = value.text();
                // @@@ This is a gross hack so I can exclude my drawer
                // entries. In reality, we should be passing the list of
                // ancestor elements into this function so we can have context
                // awareness.
                if value.to_lowercase().contains(":id:") {
                    return ()
                }

                self.exporter.push_str(value.to_string())
            },
            Event::Enter(Container::Code( _value )) | Event::Leave(Container::Code( _value )) => {
                self.exporter.push_str("`")
            },
            Event::Enter(Container::SourceBlock(_)) => {
                self.exporter.push_str("\n(subtext does not yet support code blocks, but this is where one would be)\n\n");
                ctx.skip();
            }
            Event::Enter(Container::Headline(headline)) => {
                self.exporter.push_str(format!("{} {}\n", "#".repeat(headline.level()),
                                               headline.title().map(|n| n.to_string()).collect::<String>() ))
            },

            // Newlines to end the element
            Event::Leave(Container::Headline(_)) => {
                self.exporter.push_str("\n")
            },
            Event::Leave(Container::Section(_)) => {
                self.exporter.push_str("\n")
            },
            Event::Enter(Container::Paragraph(p)) => {
                if p.syntax().ancestors().any(|t| {
                    return t.kind() == SyntaxKind::QUOTE_BLOCK;
                }) {
                    self.exporter.push_str("> ");
                };
            },
            // TODO: Paragraph should also skip the ending tags if it's in the decendents of a drawer
            Event::Leave(Container::Paragraph(p)) => {
                if p.syntax().ancestors().any(|t| {
                    t.kind() == SyntaxKind::LIST
                }) {
                    ctx.r#continue();
                    return
                }

                // If we have are in a quote block and there is another quote
                // block after us, add in a > representing the blank line.
                if p.syntax().ancestors().any(|t| {
                    return t.kind() == SyntaxKind::QUOTE_BLOCK;
                }) && p.syntax().next_sibling().is_some() {
                    self.exporter.push_str(">");
                }

                self.exporter.push_str("\n");

            },

            Event::Enter(Container::ListItem(_item)) => {
                // NOTE: subtext only supports a single item depth on lists
                self.exporter.push_str("- ")
            },
            Event::Enter(Container::Keyword(k)) => {
                if k.key() == "title" {
                    let title: &str = &k.value();
                    // need to deref the value to get it as a string.
                    self.exporter.push_str(format!("# {}\n", title.trim()));
                }
            },
            Event::Enter(Container::Link(link)) => {
                let path = link.path();
                if path.starts_with("id:") {
                    let quoted_link_id = String::from(format!(r#""{}""#, &path[3..]));
                    // TODO: output debugging info for the other files we should convert if we want it to all link correctly.
                    // TODO: Memoize this for common links.
                    let title = match executor::block_on(async {
                        sqlx::query!(r#"
SELECT title as "title!: String"
FROM nodes
WHERE id = ?
LIMIT 1
"#, quoted_link_id)
                            .fetch_one(&self.sqlite)
                            .await
                    }) {
                        Ok(result) => result.title,
                        Err(msg) => {
                            println!("Warning: Unable to find link {} due to error {}", quoted_link_id, msg);
                            String::new()
                        },
                    };
                    self.exporter.push_str(format!("[[{}]]", unquote(title.as_str())));

                } else {
                    let text = link.syntax().children_with_tokens().map(|c| {
                        if c.kind() == SyntaxKind::TEXT {
                            return c.as_token().expect("Text elements should have token values").text().to_string();
                        }
                        return String::new();
                    }).reduce(|acc,e | { acc + &e });

                    match text {
                        Some(text) => self.exporter.push_str(format!("{} <{}>", text, path.to_string())),
                        _ => self.exporter.push_str(format!("<{}>", path.to_string())),
                    }
                }

                ctx.skip();

            },
            _ => (),
        };

    }
}



#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut f = File::create(&args.output).expect(format!("Couldn't create file: {}", &args.output).as_str());

    let contents = fs::read_to_string(args.filename)?;
    let pool = SqlitePool::connect(
        &env::var("DATABASE_URL")
            .expect("You need to specify the DATABASE_URL envvar for your sqlite db. It's probably `sqlite:~/.emacs.d/org-roam.db`")
    ).await?;

    let tree = Org::parse(&contents);
    if args.debug {
        println!("{:#?}", tree.document().syntax());
    }
    let mut subtext = SubtextExporter::new(pool, HtmlExport::default());

    // @@@ Why is the handler mutable?
    // @@ Why no error possibilities here?
    tree.traverse(&mut subtext);

    write!(f, "{}", subtext.exporter.finish())?;

    Ok(())
}
