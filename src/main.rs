use std::env;
use std::io::{Error, Write};
use std::fs::{self, File};

use serde_json::to_string_pretty;
use futures::executor;
use orgize::{Org, export::{TraversalContext, Traverser, HtmlExport}, forward_handler,
             ast::{Headline,ListItem}};

// TODO: I shouldn't have to import this for the forward_handler macro to work?
use orgize::rowan::WalkEvent;
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
    fn headline(&mut self, event: WalkEvent<&Headline>, ctx: &mut TraversalContext) {
        if let WalkEvent::Enter(headline) = event {
            self.exporter.push_str(format!("{}", "#".repeat(headline.level())));
        }
    }

    fn list_item(&mut self, event: WalkEvent<&ListItem>, ctx: &mut TraversalContext) {
        if let WalkEvent::Enter(item) = event {
            self.exporter.push_str("- ")
        }
    }

    forward_handler! {
        HtmlExport,
        link text document paragraph section rule comment
        inline_src inline_call code bold verbatim italic strike underline list
        special_block quote_block center_block verse_block comment_block example_block export_block
        source_block babel_call clock cookie radio_target drawer dyn_block fn_def fn_ref macros
        snippet timestamp target fixed_width org_table org_table_row org_table_cell latex_fragment
        latex_environment entity line_break superscript subscript keyword property_drawer
    }
}

// impl ExportHandler<Error> for SubtextExporter {
//     fn start<W: Write>(&mut self, mut writer: W, element: &Element, ancestors: Vec<&Element>) -> Result<(), Error> {
//         match element {
//             Element::Text { value } => {
//                 // @@@ This is a gross hack so I can exclude my drawer
//                 // entries. In reality, we should be passing the list of
//                 // ancestor elements into this function so we can have context
//                 // awareness.
//                 if value.to_lowercase().contains(":id:") {
//                     return Ok(())
//                 }

//                 let mut val = value.clone().into_owned();

//                 // In quote blocks, we prefix all lines with a >. The QuoteBlock
//                 // element adds the first, but this code is responsible for the adding any within the quote text itself.
//                 if ancestors.iter().any(|e| match e {
//                     Element::QuoteBlock(_) => true,
//                     _ => false,
//                 }) {
//                     val = value.replace("\n", "\n> ");
//                 }

//                 write!(writer, "{}", val)?
//             },
//             Element::Code { value } => {
//                 write!(writer, "`{}`", value)?
//             },
//             Element::QuoteBlock(_) => {
//                 // @@@ Another hack due to not having the ancestry. Quote blocks
//                 // contain one or more paragraphs. With this approach, we only
//                 // quote the first paragraph.
//                 write!(writer, "> ")?
//             },
//             Element::SourceBlock(_) => {
//                 write!(writer, "\n(subtext does not yet support code blocks, but this is where one would be)\n\n")?
//             }
//             Element::Title(title) => {
//                 write!(writer, "{} ", "#".repeat(title.level))?
//             },
//             Element::ListItem(_item) => {
//                 // NOTE: subtext only supports a single item depth on lists
//                 write!(writer, "- ")?
//             },
//             Element::Keyword(k) => {
//                 if k.key == "title" {
//                     write!(writer, "# {}\n", k.value)?;
//                 }
//             },
//             Element::Link(link) => {
//                 if link.path.starts_with("id:") {
//                     let quoted_link_id = String::from(format!(r#""{}""#, &link.path[3..]));
//                     // TODO: output debugging info for the other files we should convert if we want it to all link correctly.
//                     // TODO: Memoize this for common links.
//                     let title = match executor::block_on(async {
//                         sqlx::query!(r#"
// SELECT title as "title!: String"
// FROM nodes
// WHERE id = ?
// LIMIT 1
// "#, quoted_link_id)
//                             .fetch_one(&self.sqlite)
//                             .await
//                     }) {
//                         Ok(result) => result.title,
//                         Err(msg) => {
//                             println!("Warning: Unable to find link {} due to error {}", quoted_link_id, msg);
//                             String::new()
//                         },
//                     };
//                     write!(writer, "[[{}]]", unquote(title.as_str()))?;

//                 } else {
//                 // @@@ Fix roam links.
//                     match link.desc.as_ref() {
//                         Some(desc) => write!(writer, "{} <{}>", desc, link.path)?,
//                         None => write!(writer, "<{}>", link.path)?,
//                     }
//                 }
//             },
//             _ => (),
//         }

//         Ok(())
//     }

//     fn end<W: Write>(&mut self, mut writer: W, element: &Element, _ancestors: Vec<&Element>) -> Result<(), Error> {
//         match element {
//             Element::Section => write!(writer, "\n")?,
//             Element::Title { .. } => write!(writer, "\n")?,
//             Element::QuoteBlock(_) => {
//                 write!(writer, "\n")?
//             },

//             // TODO: Paragraph should also skip the ending tags if it's in the decendents of a drawer
//             Element::Paragraph { .. } => write!(writer, "\n")?,
//             _ => (),

//         }
//         Ok(())
//     }
// }

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let f = File::create(&args.output).expect(format!("Couldn't create file: {}", &args.output).as_str());

    let contents = fs::read_to_string(args.filename)?;
    let pool = SqlitePool::connect(
        &env::var("DATABASE_URL")
            .expect("You need to specify the DATABASE_URL envvar for your sqlite db. It's probably `sqlite:~/.emacs.d/org-roam.db`")
    ).await?;

    let tree = Org::parse(&contents);
    // if args.debug {
    //     println!("{}", to_string_pretty(&tree).unwrap());
    // }
    let mut subtext = SubtextExporter::new(pool, HtmlExport::default());

    // @@@ Why is the handler mutable?
    // @@ Why no error possibilities here?
    tree.traverse(&mut subtext);

    println!("{}", subtext.exporter.finish());

    Ok(())
}
