use std::env;
use std::io::{self, Error, Write};
use std::fs;

use futures::executor;
use orgize::{Org, elements::Element, export::ExportHandler,};
use sqlx::sqlite::SqlitePool;

use clap::Parser;

/// A program to convert org-roam documents into subtext.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// File path to the org file to convert
    #[arg(short, long)]
    filename: String,
}


pub struct SubtextExporter {
    sqlite: SqlitePool,
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

// DB values somehow come back extra quoted, so provide a utility to strip it
fn unquote(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

fn wikify_string(s: &str) -> String {
    let s = s.replace(&['-', '(', ')', ',', '\"', '.', ';', ':', '\''][..], " ");
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut output = String::new();
    for word in words.iter() {
        output.push_str(capitalize(word).as_str());
    }
    output
}

impl SubtextExporter {
    fn new(pool: SqlitePool) -> SubtextExporter {
        SubtextExporter {
            sqlite: pool,
                // find_title_by_id: &conn.prepare("select title from notes where id = '\":id\"' limit 1;").unwrap(),
        }
    }
}

impl ExportHandler<Error> for SubtextExporter {
    fn start<W: Write>(&mut self, mut writer: W, element: &Element) -> Result<(), Error> {
        match element {
            Element::Text { value } => write!(writer, "{}", value)?,
            Element::Title(title) => {
                write!(writer, "{} ", "#".repeat(title.level))?
            },
            Element::Keyword(k) => {
                if k.key == "title" {
                    write!(writer, "# {}\n", k.value)?;
                }
            },
            Element::Link(link) => {
                if link.path.starts_with("id:") {
                    let quoted_link_id = String::from(format!(r#""{}""#, &link.path[3..]));
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
                    write!(writer, "[[{}]]", wikify_string(unquote(title.as_str())))?;

                } else {
                // @@@ Fix roam links.
                    match link.desc.as_ref() {
                        Some(desc) => write!(writer, "{} <{}>", desc, link.path)?,
                        None => write!(writer, "<{}>", link.path)?,
                    }
                }
            },
            _ => (),
        }

        Ok(())
    }

    fn end<W: Write>(&mut self, mut writer: W, element: &Element) -> Result<(), Error> {
        match element {
            Element::Section => write!(writer, "\n")?,
            Element::Title { .. } => write!(writer, "\n")?,
            Element::Paragraph { .. } => write!(writer, "\n")?,
            _ => (),

        }
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let contents = fs::read_to_string(args.filename)?;
    let pool = SqlitePool::connect(
        &env::var("DATABASE_URL")
            .expect("You need to specify the DATABASE_URL envvar for your sqlite db. It's probably `sqlite:~/.emacs.d/org-roam.db`")
    ).await?;

    let tree = Org::parse(&contents);
    let mut subtext = SubtextExporter::new(pool);

    // @@@ Why is the handler mutable?
    tree.write(&mut io::stdout(), &mut subtext).unwrap();

    Ok(())
}
