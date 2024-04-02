use std::{borrow::Cow, cmp, fs::File, io::Write, ops::ControlFlow};

use crossterm::event::{KeyCode, KeyEvent};
use rand::prelude::*;
use ratatui::{prelude::*, style::Stylize, widgets::*};
use serde::{Deserialize, Serialize};

use crate::{Draw, Library, Mark, Power, SaveFile};

const CONT: ControlFlow<()> = ControlFlow::Continue(());
const BREAK: ControlFlow<()> = ControlFlow::Break(());

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tab {
    DraftCreation,
    Results,
}

pub struct UiState<'a> {
    pub library: &'a mut Library,
    pub terminal: &'a mut crate::Terminal,
    save_box: Prompt<'static>,
    is_saving: bool,
    draft_view: DraftView,
    tab: Tab,
    results: Results,
    rng: ThreadRng,
}

pub struct DraftView {
    pub selected_tab: Pane,
    pub mark_list: MarkList,
    pub draft: DraftEditor,
}

impl<'a> UiState<'a> {
    pub fn new(
        library: &'a mut Library,
        terminal: &'a mut crate::Terminal,
        results: Results,
    ) -> Self {
        let len = library.list.len();
        UiState {
            library,
            terminal,
            results,
            save_box: Prompt {
                title: Line::raw("Save as"),
                postfix: Span::raw(".json"),
                max_width: 32,
                ..Default::default()
            },
            is_saving: false,
            draft_view: DraftView::new(len),
            tab: Tab::DraftCreation,
            rng: rand::thread_rng(),
        }
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        let library = self.library.clone();
        let results = self.results.clone();

        let save = SaveFile { library, results };

        Ok(())
    }

    pub fn input(&mut self, ev: KeyEvent) -> anyhow::Result<ControlFlow<()>> {
        match ev.code {
            KeyCode::Char('s' | 'S') => {
                self.is_saving = true;
            }
            k if self.is_saving => {
                let res = self.save_box.input(ev);
                self.is_saving = match res {
                    ControlFlow::Continue(_) => true,
                    ControlFlow::Break(b) => {
                        if b {
                            save(&self.library, &self.results, &self.save_box.text)?;
                        }
                        false
                    }
                };
            }
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(BREAK),
            KeyCode::Char('d' | 'D') => {
                self.tab = Tab::DraftCreation;
            }
            KeyCode::Char('r' | 'R') => {
                self.tab = Tab::Results;
            }
            KeyCode::Enter
                if self.draft_view.selected_tab == Pane::Left && self.tab == Tab::DraftCreation =>
            {
                let marks = self
                    .library
                    .exec_draw(self.draft_view.draft.draws.clone(), &mut self.rng);
                self.results
                    .results
                    .push((marks, self.draft_view.draft.draws.clone()));
                self.tab = Tab::Results;
                self.results
                    .state
                    .select(Some(self.results.results.len() - 1));
            }
            _ if self.tab == Tab::DraftCreation => {
                return Ok(self.draft_view.input(&mut self.library, ev))
            }
            k if self.tab == Tab::Results => {
                self.results.input(k);
            }
            _ => {}
        }

        Ok(CONT)
    }

    pub fn draw(&mut self) -> anyhow::Result<()> {
        let term = &mut self.terminal;

        term.clear()?;
        term.draw(|f| {
            let layout = Layout::new(
                Direction::Vertical,
                [Constraint::Length(3), Constraint::Fill(1)],
            )
            .split(f.size());
            let tabs = Tabs::new([
                Line::default().spans(["D".underlined().red(), Span::raw("raft")]),
                Line::default().spans(["R".underlined().red(), Span::raw("esults")]),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            )
            .select(match self.tab {
                Tab::DraftCreation => 0,
                Tab::Results => 1,
            });
            f.render_widget(tabs, layout[0]);
            let block2 = Block::new()
                .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
                .border_type(BorderType::Rounded);
            let inner = block2.inner(layout[1]);
            f.render_widget(block2, layout[1]);

            match self.tab {
                Tab::DraftCreation => self.draft_view.draw(&*self.library, f, inner),
                Tab::Results => self.results.draw(f, inner),
            }

            if self.is_saving {
                self.save_box.draw(f, f.size());
            }
        })?;

        Ok(())
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Results {
    results: Vec<(Vec<Mark>, Vec<Draw>)>,
    #[serde(skip)]
    state: ListState,
}

impl Results {
    fn next_selection(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.results.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn prev_selection(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.results.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up => self.prev_selection(),
            KeyCode::Down => self.next_selection(),
            _ => {}
        }
    }

    pub fn draw(&mut self, f: &mut Frame, rect: Rect) {
        let layout = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(15),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ],
        )
        .split(rect);
        let draft_list = List::new(
            self.results
                .iter()
                .enumerate()
                .map(|(c, _)| format!("Draft #{c}")),
        )
        .block(Block::bordered().border_type(BorderType::Rounded))
        .highlight_symbol(">>")
        .highlight_spacing(HighlightSpacing::Always);

        if draft_list.is_empty() {
            f.render_widget(
                Paragraph::new("<empty>".italic().dark_gray())
                    .block(Block::bordered().border_type(BorderType::Rounded))
                    .centered(),
                layout[0],
            );
            f.render_widget(
                Block::bordered().border_type(BorderType::Rounded),
                layout[1],
            );
        } else {
            f.render_stateful_widget(draft_list, layout[0], &mut self.state);
            let (mark_list, draws) = match self.state.selected() {
                Some(i) => self.results[i].clone(),
                None => (vec![], vec![]),
            };

            let listing = List::new(mark_list.iter().map(|m| {
                let power_span = power_str(m.power);
                m.name.as_str().set_style(power_span.style)
            }))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .padding(Padding {
                        left: 4,
                        top: 1,
                        ..Default::default()
                    }),
            );

            let editor = DraftEditor {
                draws,
                line: 0,
                scroll: 0,
            };
            let draw =
                editor
                    .draw()
                    .block(
                        Block::bordered()
                            .border_type(BorderType::Rounded)
                            .padding(Padding {
                                left: 4,
                                top: 1,
                                ..Default::default()
                            }),
                    );

            f.render_widget(listing, layout[1]);
            f.render_widget(draw, layout[2]);
        }
    }
}

impl DraftView {
    pub fn new(n_marks: usize) -> Self {
        DraftView {
            selected_tab: Pane::Left,
            mark_list: MarkList::new(n_marks),
            draft: DraftEditor::default(),
        }
    }

    pub fn input(&mut self, lib: &mut Library, ev: KeyEvent) -> ControlFlow<()> {
        let cont = ControlFlow::Continue(());

        match ev.code {
            KeyCode::Tab => {
                self.selected_tab = match self.selected_tab {
                    Pane::Left => Pane::Right,
                    Pane::Right => Pane::Left,
                };
                cont
            }
            k if self.selected_tab == Pane::Left => {
                self.draft.input(lib, k);
                cont
            }
            k if self.selected_tab == Pane::Right => {
                self.mark_list.input(lib, k);
                cont
            }
            _ => cont,
        }
    }

    pub fn draw(&mut self, lib: &Library, f: &mut Frame, rect: Rect) {
        let inactive_tab = Style::default().fg(Color::DarkGray);
        let active_tab = Style::default();

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(&[Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rect);

        let left_block = Block::default()
            .title("Draft")
            .borders(Borders::ALL)
            .border_style(match self.selected_tab {
                Pane::Right => inactive_tab,
                Pane::Left => active_tab,
            })
            .padding(Padding {
                left: 3,
                right: 0,
                top: 1,
                bottom: 0,
            })
            .border_type(BorderType::Rounded);
        let rect = left_block.inner(cols[0]);
        f.render_widget(left_block, cols[0]);

        let mark_draft = self.draft.draw();
        f.render_widget(mark_draft, rect);

        let mark_block = Block::default()
            .title("Marks")
            .borders(Borders::ALL)
            .border_style(match self.selected_tab {
                Pane::Left => inactive_tab,
                Pane::Right => active_tab,
            })
            .border_type(ratatui::widgets::BorderType::Rounded);
        let mark_inner = mark_block.inner(cols[1]);
        f.render_widget(mark_block, cols[1]);

        self.mark_list.draw(lib, f, mark_inner);
    }
}

#[derive(Default)]
pub struct DraftEditor {
    draws: Vec<Draw>,
    line: usize,
    scroll: usize,
}

fn draw_lines(draw: &Draw) -> usize {
    1 + draw.power.is_some() as usize + draw.category.is_some() as usize + draw.tags.len()
}

#[derive(Copy, Clone, Debug)]
enum Dir {
    Left,
    Right,
}

#[derive(Copy, Clone, Debug)]
enum ElementKind {
    Mark,
    Power,
    Category,
    Tag(usize),
}

impl DraftEditor {
    pub fn input(&mut self, lib: &Library, key: KeyCode) {
        match key {
            KeyCode::Down => self.line = cmp::min(self.max_line().saturating_sub(1), self.line + 1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = cmp::min(self.scroll + 1, self.max_line()),
            KeyCode::Up => self.line = self.line.saturating_sub(1),
            KeyCode::Left if self.draws.len() > 0 => self.rotate_current_element(lib, Dir::Left),
            KeyCode::Right if self.draws.len() > 0 => self.rotate_current_element(lib, Dir::Right),
            KeyCode::Backspace if self.draws.len() > 0 => self.delete_current_element(),
            KeyCode::Char('a' | 'A') => self.add_plain_mark(),
            KeyCode::Char('c' | 'C') if self.draws.len() > 0 => self.add_or_modify_category(lib),
            KeyCode::Char('p' | 'P') if self.draws.len() > 0 => self.add_or_modify_power(),
            KeyCode::Char('t' | 'T') if self.draws.len() > 0 => self.add_tag(lib),
            _ => {}
        }
    }

    pub fn max_line(&self) -> usize {
        self.draws.iter().map(draw_lines).sum()
    }

    pub fn add_plain_mark(&mut self) {
        self.draws.push(Draw::default());
    }

    pub fn add_or_modify_power(&mut self) {
        self.get_selected_draw().power = Some(Power::Supreme);
    }

    pub fn get_selected_draw(&mut self) -> &mut Draw {
        self.get_selection().0
    }

    pub fn get_selection(&mut self) -> (&mut Draw, usize, usize) {
        let mut cur_draw = (
            0,
            draw_lines(
                self.draws
                    .get(0)
                    .expect("Tried to get selected draw with no draws in the draft"),
            ),
        );
        let mut i = 0;
        while !(self.line >= cur_draw.0 && self.line < cur_draw.1) {
            i += 1;
            let next_draw = &self.draws[i];
            cur_draw = (cur_draw.1, cur_draw.1 + draw_lines(next_draw));
        }

        (&mut self.draws[i], self.line - cur_draw.0, i)
    }

    fn add_or_modify_category(&mut self, lib: &Library) {
        self.get_selected_draw().category = Some(lib.categories.iter().nth(0).unwrap().clone());
    }

    fn get_element_kind(&mut self) -> ElementKind {
        let (draw, offset, _) = self.get_selection();
        let mut v = vec![ElementKind::Mark];
        if draw.power.is_some() {
            v.push(ElementKind::Power);
        }
        if draw.category.is_some() {
            v.push(ElementKind::Category);
        }
        for (c, _) in draw.tags.iter().enumerate() {
            v.push(ElementKind::Tag(c));
        }
        v[offset]
    }

    fn rotate_current_element(&mut self, lib: &Library, dir: Dir) {
        let element_kind = self.get_element_kind();
        eprintln!("{:?}", element_kind);
        let draw = self.get_selected_draw();

        fn find_and_rotate<T: PartialEq>(x: &T, mut v: Vec<T>, dir: Dir) -> T {
            while &v[0] != x {
                v.rotate_right(1);
            }
            match dir {
                Dir::Left => v.rotate_left(1),
                Dir::Right => v.rotate_right(1),
            }
            v.swap_remove(0)
        }

        if let ElementKind::Power = element_kind {
            let powers = [
                Power::BadKarma,
                Power::Poor,
                Power::Moderate,
                Power::Good,
                Power::Great,
                Power::Supreme,
                Power::Unique,
            ];
            let p = draw.power.unwrap();

            draw.power = Some(find_and_rotate(&p, powers.to_vec(), dir));
        }

        if let ElementKind::Category = element_kind {
            let categories: Vec<_> = lib.categories.iter().cloned().collect();
            let category = draw.category.as_ref().unwrap();

            draw.category = Some(find_and_rotate(&category, categories, dir));
        }

        if let ElementKind::Tag(n) = element_kind {
            let mut tags = lib.tags.clone();
            let mut existing_tags = draw.tags.clone();
            existing_tags.remove(n);
            let tag = &draw.tags[n];
            for tag in existing_tags {
                tags.remove(&tag);
            }
            let tags: Vec<_> = tags.into_iter().collect();

            draw.tags[n] = find_and_rotate(tag, tags, dir);
        }
    }

    fn delete_current_element(&mut self) {
        let element_kind = self.get_element_kind();
        let (draw, _, idx) = self.get_selection();
        if let ElementKind::Mark = element_kind {
            let _ = draw;
            self.draws.remove(idx);
        } else {
            match element_kind {
                ElementKind::Mark => {}
                ElementKind::Power => draw.power = None,
                ElementKind::Category => draw.category = None,
                ElementKind::Tag(n) => {
                    draw.tags.remove(n);
                }
            }
        }
        self.line = self.line.saturating_sub(1);
    }

    fn add_tag(&mut self, library: &Library) {
        let draw = self.get_selected_draw();
        let mut tag_lib = library.tags.clone();
        let existing_tags = draw.tags.clone();
        for tag in existing_tags {
            tag_lib.remove(&tag);
        }

        if !tag_lib.is_empty() {
            draw.tags.push(tag_lib.iter().nth(0).unwrap().clone())
        }
    }

    pub fn draw(&self) -> Paragraph<'_> {
        let mut i = 0;
        let mut style_line = || {
            let style = if i == self.line {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            i += 1;
            style
        };

        let mut text = Text::from(vec![]);

        for (c, draw) in self.draws.iter().enumerate() {
            text.extend(format_draw(draw, c, &mut style_line))
        }

        Paragraph::new(text).scroll((self.scroll as u16, 0))
    }
}

fn format_draw<'a, F: FnMut() -> Style>(
    draw: &'a Draw,
    n: usize,
    mut style_line: F,
) -> Vec<Line<'a>> {
    let mut v = vec![];
    v.push(Line::styled(
        format!("Draw {}", n + 1),
        style_line().fg(Color::Red),
    ));
    if let Some(p) = &draw.power {
        v.push(label_text_span(">> Power", power_str(*p)).style(style_line()));
    }
    if let Some(c) = &draw.category {
        v.push(label_text_span(">> Category", Span::raw(c.as_str())).style(style_line()));
    }
    for tag in &draw.tags {
        v.push(label_text_span(">> Tag", Span::raw(tag.as_str())).style(style_line()));
    }
    v
}

pub struct MarkList {
    state: TableState,
    n_items: usize,
}

impl MarkList {
    pub fn new(n_items: usize) -> Self {
        Self {
            state: TableState::default(),
            n_items,
        }
    }

    pub fn input(&mut self, lib: &mut Library, code: KeyCode) {
        match code {
            KeyCode::Up => self.prev_mark(),
            KeyCode::Down => self.next_mark(),
            KeyCode::Enter => {
                let Some(i) = self.state.selected() else {
                    return;
                };
                lib.list[i].1 = !lib.list[i].1;
            }
            _ => {}
        }
    }

    pub fn draw(&mut self, library: &Library, f: &mut Frame, area: Rect) {
        let layout = Layout::new(
            Direction::Vertical,
            [Constraint::Percentage(60), Constraint::Percentage(40)],
        )
        .spacing(1)
        .split(area);

        let longest_name = library
            .list
            .iter()
            .map(|(m, _)| m.name.len())
            .max()
            .unwrap();
        let longest_cat = library.categories.iter().map(|c| c.len()).max().unwrap();
        let longest_tags = library
            .list
            .iter()
            .map(|(m, _)| m.tags.iter().map(|s| s.len()).intersperse(2).sum::<usize>())
            .max()
            .unwrap();

        let mark_table = Table::new(
            library
                .list
                .iter()
                .map(|(mark, free)| {
                    Row::new([
                        Span::styled(
                            mark.name.as_str(),
                            if !*free {
                                Style::default().crossed_out()
                            } else {
                                Style::default()
                            },
                        ),
                        power_str(mark.power),
                        Span::raw(mark.category.clone()),
                        Span::raw(
                            mark.tags
                                .iter()
                                .map(|s| s.as_str())
                                .intersperse(", ")
                                .collect::<String>(),
                        ),
                    ])
                })
                .collect::<Vec<_>>(),
            [
                Constraint::Length(longest_name as u16),
                Constraint::Length(8),
                Constraint::Length(cmp::max(longest_cat as u16, 8)),
                Constraint::Length(longest_tags as u16),
            ],
        )
        .header(Row::new([
            "Name".underlined(),
            "Power".underlined(),
            "Category".underlined(),
            "Tags".underlined(),
        ]))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">>");

        let selected_mark = &library.list[self.state.selected().unwrap_or(0)].0;

        let tag_text: String = selected_mark
            .tags
            .iter()
            .map(String::as_str)
            .intersperse(", ")
            .collect();

        let mut text = Text::from(vec![
            label_text_span("Power", power_str(selected_mark.power)),
            label_text_span("Category", selected_mark.category.as_str().reset()),
            label_text_span("Tags", tag_text.reset()),
            Line::styled(
                "Description",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]);
        text.extend(Text::raw(selected_mark.description.as_str()));

        let description_box = Paragraph::new(text)
            .block(
                Block::default()
                    .title(selected_mark.name.clone().bold())
                    .borders(Borders::all())
                    .border_type(BorderType::Rounded),
            )
            .wrap(Wrap { trim: true });
        f.render_stateful_widget(mark_table, layout[0], &mut self.state);
        f.render_widget(description_box, layout[1])
    }

    fn next_mark(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.n_items - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn prev_mark(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.n_items - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

fn label_text_span<'a>(label: &'a str, text: Span<'a>) -> Line<'a> {
    Line::from(vec![
        Span::styled(label, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(": ", Style::default().add_modifier(Modifier::BOLD)),
        text,
    ])
}

fn power_str(p: Power) -> Span<'static> {
    match p {
        Power::Poor => "Poor".dark_gray(),
        Power::Moderate => "Moderate".white(),
        Power::Good => "Good".green(),
        Power::Great => "Great".cyan(),
        Power::Supreme => "Supreme".red(),
        Power::Unique => "Unique".magenta(),
        Power::BadKarma => "Bad Karma".black().on_red().bold(),
    }
}

#[derive(Clone, Debug, Default)]
struct Prompt<'a> {
    pub text: String,
    pub title: Line<'a>,
    pub prefix: Span<'a>,
    pub postfix: Span<'a>,
    pub cursor_pos: usize,
    pub max_width: usize,
}

impl<'a> Prompt<'a> {
    fn input(&mut self, ev: KeyEvent) -> ControlFlow<bool> {
        match ev.code {
            KeyCode::Esc => return ControlFlow::Break(false),
            KeyCode::Enter => return ControlFlow::Break(true),
            KeyCode::Char(c) if c.is_ascii() => {
                self.text.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
            }
            KeyCode::Backspace if self.cursor_pos > 0 && self.text.len() > 0 => {
                self.text.remove(self.cursor_pos - 1);
                self.cursor_pos -= 1;
            }
            KeyCode::Right => self.cursor_pos = cmp::min(self.cursor_pos + 1, self.max_width - 1),
            KeyCode::Left => self.cursor_pos = self.cursor_pos.saturating_sub(1),
            _ => {}
        }

        ControlFlow::Continue(())
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Fill(1),
        ])
        .split(area);

        let area = layout[1];

        let mut par_text = Line::default().spans([
            self.prefix.clone(),
            Span::raw(format!(
                "{content:_<width$}",
                content = self.text,
                width = self.max_width,
            )),
            self.postfix.clone(),
        ]);

        let width = par_text.width() + 4;

        let layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(width as u16),
            Constraint::Fill(1),
        ])
        .split(area);

        let area = layout[1];

        let mut text = Text::from(par_text);

        // left side + border + pad + prefix len + cursor_pos + one after
        let cursor_x = area.x + 2 + self.prefix.content.len() as u16 + self.cursor_pos as u16;
        let cursor_y = area.y + 1;

        f.set_cursor(cursor_x, cursor_y);

        let par = Paragraph::new(text)
            .centered()
            .block(Block::bordered().title(self.title.clone()));

        f.render_widget(Clear, area);
        f.render_widget(par, area);
    }
}

fn save(library: &Library, results: &Results, filename: &str) -> anyhow::Result<()> {
    let library = library.clone();
    let results = results.clone();
    let savefile = SaveFile { library, results };

    let save = format!("{}.json", filename);

    let mut f = File::create(save)?;

    serde_json::to_writer(&mut f, &savefile)?;

    f.flush()?;

    Ok(())
}
