#![feature(iter_intersperse)]

use anyhow::{bail, format_err};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use rand::prelude::*;
use ratatui::backend::CrosstermBackend;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, env, fs::File, io, ops::ControlFlow, path::Path};

type Terminal = ratatui::Terminal<CrosstermBackend<io::Stdout>>;

mod ui;

use ui::{Results, UiState};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct Library {
    list: Vec<(Mark, bool)>,
    categories: BTreeSet<String>,
    tags: BTreeSet<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct SaveFile {
    library: Library,
    results: Results,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Mark {
    name: String,
    power: Power,
    category: String,
    tags: BTreeSet<String>,
    description: String,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
enum Power {
    BadKarma,
    Poor,
    #[default]
    Moderate,
    Good,
    Great,
    Supreme,
    Unique,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Draw {
    power: Option<Power>,
    category: Option<String>,
    tags: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let arg_err = || {
        format_err!("You need to provide a path to a library csv/saved json to run this program")
    };

    env_logger::init();

    let library_file_name = env::args().nth(1).ok_or(arg_err())?;

    let library_file_name = Path::new(&library_file_name);
    // this path came from a string so we unwrap directly
    let ext = library_file_name
        .extension()
        .ok_or(arg_err())?
        .to_str()
        .unwrap();

    let mut save: SaveFile = match ext {
        "csv" => SaveFile::parse_library_file(&library_file_name)?,
        "json" => {
            let f = File::open(library_file_name)?;
            serde_json::from_reader(f)?
        }
        _ => bail!("Unknown library extension {ext}"),
    };

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_eventloop(save, &mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run_eventloop(save: SaveFile, terminal: &mut Terminal) -> anyhow::Result<()> {
    let SaveFile {
        mut library,
        results: past_results,
    } = save;

    let mut state = UiState::new(&mut library, terminal, past_results);

    state.draw()?;

    loop {
        let ev = event::read()?;

        match ev {
            Event::Key(ev) => match state.input(ev)? {
                ControlFlow::Break(_) => break,
                ControlFlow::Continue(_) => {}
            },
            _ => {}
        }

        state.draw()?;
    }

    Ok(())
}

impl Library {
    pub fn exec_draw(&mut self, draws: Vec<Draw>, rng: &mut ThreadRng) -> Vec<Mark> {
        let mut pool = Vec::new();

        let mut marks: Vec<Mark> = Vec::new();

        for draw in draws {
            'mark: for (mark, free) in &self.list {
                if !free {
                    continue;
                }
                if draw.power.as_ref().is_some_and(|p| match (*p, mark.power) {
                    (x, y) if x == y => false,
                    (Power::BadKarma, Power::Poor | Power::Moderate) => false,
                    _ => true,
                }) {
                    continue;
                }
                if draw.category.as_ref().is_some_and(|c| &mark.category != c) {
                    continue;
                }
                for tag in &draw.tags {
                    if !mark.tags.contains(tag) {
                        continue 'mark;
                    }
                }
                if marks.iter().find(|m| m.name == mark.name).is_some() {
                    continue;
                }

                pool.push(mark);
            }

            let choice = pool.choose(rng).map(|m| (**m).clone()).unwrap_or(Mark {
                name: "STUPID".to_string(),
                power: Power::Poor,
                ..Default::default()
            });
            marks.push(choice);
            pool.clear()
        }

        marks
    }
}

impl SaveFile {
    fn parse_library_file<S: AsRef<Path>>(path: S) -> anyhow::Result<Self> {
        // NAME,POWER,CATEGORY,TAG,TAG,DESCRIPTION

        let mut rdr = csv::Reader::from_path(path)?;
        let tag_count = rdr.headers()?.iter().filter(|f| f == &"TAG").count();
        let mut v = Vec::new();

        let mut categories = BTreeSet::new();
        let mut all_tags = BTreeSet::new();

        for result in rdr.into_records() {
            use Power as P;

            let record = result?;
            let mut fields = record.iter();
            let mut next = || {
                fields
                    .next()
                    .ok_or(anyhow::Error::msg("Malformed library csv"))
            };

            let name = next()?.to_string();
            let power = match next()? {
                "Poor" => P::Poor,
                "Moderate" => P::Moderate,
                "Good" => P::Good,
                "Great" => P::Great,
                "Supreme" => P::Supreme,
                "Unique" => P::Unique,
                "Bad Karma" => P::BadKarma,
                e => bail!("Unknown power level {:?}", e),
            };

            let category = next()?.to_string();
            if !categories.contains(&category) && category != "" {
                categories.insert(category.clone());
            }

            let mut tags = BTreeSet::new();
            for _ in 0..tag_count {
                match next()? {
                    "" => continue,
                    t => {
                        tags.insert(t.to_string());
                        if !all_tags.contains(t) {
                            all_tags.insert(t.to_string());
                        }
                    }
                }
            }

            let description = next()?.to_string();

            let mark = Mark {
                name,
                power,
                category,
                tags,
                description,
            };

            v.push((mark, true));
        }

        Ok(SaveFile {
            library: Library {
                list: v,
                categories,
                tags: all_tags,
            },
            ..Default::default()
        })
    }
}
