//! Scrolling for text panes: an offset that follows the tail until the reader scrolls
//! away, and a cache of wrapped row counts so a long transcript costs only what is on
//! screen.
//!
//! The follow logic is rust-code's chat panel (`chat_scroll` / `chat_pinned` /
//! `chat_total_lines` in rc-cli): render reports how tall the content is, a pinned view
//! snaps to the bottom, scrolling up unpins, scrolling back down to the bottom re-pins.
//! The keys are its content viewer's (arrows and j/k, PgUp/PgDn by a viewport, Home/End
//! and g/G). Row counts come from ratatui's own word wrapper (`Paragraph::line_count`),
//! so the estimate and the drawing can never disagree.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

/// Rows a mouse wheel notch moves (rust-code uses 3).
pub const WHEEL_ROWS: usize = 3;

/// Scroll position of one pane.
#[derive(Debug, Clone, Default)]
pub struct ScrollView {
    offset: usize,
    total: usize,
    viewport: usize,
    follow: bool,
}

impl ScrollView {
    /// Starts pinned to the tail (a live transcript).
    pub fn following() -> Self {
        Self {
            follow: true,
            ..Self::default()
        }
    }

    /// Starts at the top and stays where the reader leaves it (a document).
    pub fn at_top() -> Self {
        Self::default()
    }

    /// Called on every render with the content height and the visible height.
    pub fn layout(&mut self, total: usize, viewport: usize) {
        self.total = total;
        self.viewport = viewport;
        self.offset = if self.follow {
            self.max_offset()
        } else {
            self.offset.min(self.max_offset())
        };
    }

    pub fn max_offset(&self) -> usize {
        self.total.saturating_sub(self.viewport)
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// Up never re-pins; it unpins as soon as the tail leaves the screen.
    pub fn up(&mut self, rows: usize) {
        self.offset = self.offset.min(self.max_offset()).saturating_sub(rows);
        self.follow = self.follow && self.offset >= self.max_offset();
    }

    /// Down re-pins on reaching the bottom.
    pub fn down(&mut self, rows: usize) {
        self.offset = (self.offset + rows).min(self.max_offset());
        self.follow = self.offset >= self.max_offset();
    }

    fn page(&self) -> usize {
        self.viewport.saturating_sub(1).max(1)
    }

    pub fn page_up(&mut self) {
        self.up(self.page());
    }

    pub fn page_down(&mut self) {
        self.down(self.page());
    }

    /// Top, and stay there: new lines no longer pull the view down.
    pub fn home(&mut self) {
        self.offset = 0;
        self.follow = false;
    }

    /// Bottom, following the tail again.
    pub fn end(&mut self) {
        self.offset = self.max_offset();
        self.follow = true;
    }

    /// Put `row` a third of the way down the pane (jump to a search match).
    pub fn reveal(&mut self, row: usize) {
        self.offset = row.saturating_sub(self.viewport / 3).min(self.max_offset());
        self.follow = false;
    }

    /// `42%` once the content is taller than the pane, else `None`.
    pub fn percent(&self) -> Option<usize> {
        let max = self.max_offset();
        (max > 0).then(|| self.offset.min(max) * 100 / max)
    }

    /// The navigation keys every pane shares. True if the key moved (or was for) the view.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => self.up(1),
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => self.down(1),
            (KeyCode::PageUp, _) | (KeyCode::Char('b'), KeyModifiers::CONTROL) => self.page_up(),
            (KeyCode::PageDown, _) | (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                self.page_down()
            }
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => self.home(),
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => self.end(),
            _ => return false,
        }
        true
    }

    /// Mouse wheel. True if it was a wheel event.
    pub fn handle_wheel(&mut self, mouse: MouseEvent) -> bool {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.up(WHEEL_ROWS),
            MouseEventKind::ScrollDown => self.down(WHEEL_ROWS),
            _ => return false,
        }
        true
    }
}

/// Rows `line` takes when word-wrapped to `width`, exactly as a wrapped `Paragraph` draws it.
pub fn line_rows(line: &Line, width: u16) -> usize {
    Paragraph::new(line.clone())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1)
}

/// Cumulative wrapped rows of an append-only list of lines at one width. Appending costs
/// the new lines only; a resize or a shorter list starts over.
#[derive(Debug, Clone, Default)]
pub struct WrappedRows {
    width: u16,
    /// `cum[i]`: rows above logical line `i`; one longer than the list.
    cum: Vec<usize>,
}

impl WrappedRows {
    /// Bring the cache up to `items` at `width`. `line` renders item `i`.
    pub fn sync<'a, T>(
        &mut self,
        width: u16,
        items: &'a [T],
        line: impl Fn(usize, &'a T) -> Line<'a>,
    ) {
        if width != self.width || self.cum.is_empty() || items.len() + 1 < self.cum.len() {
            self.width = width;
            self.cum.clear();
            self.cum.push(0);
        }
        let done = self.cum.len() - 1;
        for (i, item) in items.iter().enumerate().skip(done) {
            let rows = line_rows(&line(i, item), width);
            let above = self.cum.last().copied().unwrap_or(0);
            self.cum.push(above + rows);
        }
    }

    /// Forget everything (the list was replaced, not appended to).
    pub fn reset(&mut self) {
        self.cum.clear();
    }

    pub fn total(&self) -> usize {
        self.cum.last().copied().unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.cum.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// First row of logical line `i`.
    pub fn row_of(&self, i: usize) -> usize {
        self.cum.get(i).copied().unwrap_or_else(|| self.total())
    }

    /// The logical lines a pane scrolled to `offset` shows, `[first, end)`, and how many rows
    /// of `first` are hidden above the pane.
    pub fn window(&self, offset: usize, viewport: usize) -> (usize, usize, usize) {
        if self.is_empty() {
            return (0, 0, 0);
        }
        let first = self
            .cum
            .partition_point(|&r| r <= offset)
            .saturating_sub(1)
            .min(self.len() - 1);
        let bottom = offset + viewport;
        let end = self.cum.partition_point(|&r| r < bottom).min(self.len());
        (first, end.max(first + 1), offset - self.cum[first])
    }
}

/// A pane's scroll state: the view and the wrap cache of what it shows.
#[derive(Debug, Clone, Default)]
pub struct Pane {
    pub view: ScrollView,
    pub rows: WrappedRows,
}

impl Pane {
    pub fn following() -> Self {
        Self {
            view: ScrollView::following(),
            rows: WrappedRows::default(),
        }
    }

    pub fn at_top() -> Self {
        Self {
            view: ScrollView::at_top(),
            rows: WrappedRows::default(),
        }
    }

    /// The content was replaced (not appended to): recount rows on the next render.
    pub fn replaced(&mut self) {
        self.rows.reset();
    }
}

/// Draw `items` in `area` scrolled to `pane.view`, building only the lines on screen.
/// `tail` is one more line after the items that is not cached (a partial transcript).
pub fn render_pane<'a, T>(
    f: &mut Frame,
    area: Rect,
    block: Block<'a>,
    pane: &mut Pane,
    items: &'a [T],
    to_line: impl Fn(usize, &'a T) -> Line<'a>,
    tail: Option<Line<'a>>,
) {
    let inner = block.inner(area);
    pane.rows.sync(inner.width, items, &to_line);
    let tail_rows = tail.as_ref().map_or(0, |l| line_rows(l, inner.width));
    let height = inner.height as usize;
    let total = pane.rows.total();
    pane.view.layout(total + tail_rows, height);
    let offset = pane.view.offset();
    let (mut lines, skip): (Vec<Line>, usize) = if offset >= total {
        (Vec::new(), offset - total)
    } else {
        let (first, end, skip) = pane.rows.window(offset, height);
        ((first..end).map(|i| to_line(i, &items[i])).collect(), skip)
    };
    if let Some(tail) = tail
        && offset + height > total
    {
        lines.push(tail);
    }
    let skip = u16::try_from(skip).unwrap_or(u16::MAX);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((skip, 0))
        .block(block);
    f.render_widget(para, area);
}

/// `42%`, or `42% · End follows` for a pane that stopped following; empty when the
/// content fits.
pub fn position_label(view: &ScrollView, follows: bool) -> String {
    match view.percent() {
        None => String::new(),
        Some(p) if follows && !view.is_following() => format!("{p}% · End follows"),
        Some(p) => format!("{p}%"),
    }
}

/// ` Transcript · 42% · match 2/9 `: the non-empty parts of a pane title.
pub fn pane_title(parts: &[&str]) -> String {
    let parts: Vec<&str> = parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    format!(" {} ", parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(total: usize, viewport: usize, follow: bool) -> ScrollView {
        let mut v = if follow {
            ScrollView::following()
        } else {
            ScrollView::at_top()
        };
        v.layout(total, viewport);
        v
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn a_following_view_sits_on_the_tail_as_content_grows() {
        let mut v = view(100, 20, true);
        assert_eq!(v.offset(), 80);
        v.layout(130, 20);
        assert_eq!(v.offset(), 110);
        assert!(v.is_following());
    }

    #[test]
    fn scrolling_up_stops_following_and_new_lines_do_not_move_the_view() {
        let mut v = view(100, 20, true);
        v.up(5);
        assert_eq!(v.offset(), 75);
        assert!(!v.is_following());
        v.layout(150, 20);
        assert_eq!(v.offset(), 75);
    }

    #[test]
    fn scrolling_back_to_the_bottom_or_end_resumes_following() {
        let mut v = view(100, 20, true);
        v.page_up();
        assert_eq!(v.offset(), 61); // a page keeps one row of context
        v.page_down();
        assert!(v.is_following());
        v.home();
        assert_eq!(v.offset(), 0);
        assert!(!v.is_following());
        v.end();
        v.layout(200, 20);
        assert_eq!(v.offset(), 180);
    }

    #[test]
    fn content_shorter_than_the_pane_never_scrolls_and_keeps_following() {
        let mut v = view(5, 20, true);
        v.up(3);
        assert_eq!(v.offset(), 0);
        assert!(
            v.is_following(),
            "a stray k on a short pane must not freeze it"
        );
        assert_eq!(v.percent(), None);
        v.down(10);
        assert_eq!(v.offset(), 0);
    }

    #[test]
    fn a_document_view_starts_at_the_top_and_keeps_its_place_on_replace() {
        let mut v = view(60, 20, false);
        assert_eq!(v.offset(), 0);
        v.down(10);
        v.layout(70, 20); // the agent rewrote summary.md
        assert_eq!(v.offset(), 10);
        v.layout(15, 20); // ...and shortened it
        assert_eq!(v.offset(), 0);
        v.end();
        v.layout(90, 20);
        assert_eq!(v.offset(), 70, "End on a document follows its tail too");
    }

    #[test]
    fn up_from_a_stale_offset_is_clamped_first() {
        let mut v = view(100, 20, false);
        v.down(80);
        v.layout(30, 20); // content shrank under the view
        v.up(1);
        assert_eq!(v.offset(), 9);
    }

    #[test]
    fn keys_map_like_rust_code_viewer() {
        let mut v = view(100, 20, true);
        assert!(v.handle_key(key(KeyCode::Char('k'))));
        assert_eq!(v.offset(), 79);
        assert!(v.handle_key(key(KeyCode::Char('g'))));
        assert_eq!(v.offset(), 0);
        assert!(v.handle_key(key(KeyCode::Char('j'))));
        assert_eq!(v.offset(), 1);
        assert!(v.handle_key(key(KeyCode::PageDown)));
        assert_eq!(v.offset(), 20);
        assert!(v.handle_key(key(KeyCode::End)));
        assert!(v.is_following());
        assert!(!v.handle_key(key(KeyCode::Char('x'))));
    }

    #[test]
    fn wheel_moves_three_rows() {
        let mut v = view(100, 20, true);
        let wheel = |kind| MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert!(v.handle_wheel(wheel(MouseEventKind::ScrollUp)));
        assert_eq!(v.offset(), 77);
        assert!(v.handle_wheel(wheel(MouseEventKind::ScrollDown)));
        assert!(v.is_following());
        assert!(!v.handle_wheel(wheel(MouseEventKind::Moved)));
    }

    #[test]
    fn reveal_puts_a_row_a_third_down() {
        let mut v = view(300, 30, true);
        v.reveal(100);
        assert_eq!(v.offset(), 90);
        assert!(!v.is_following());
        v.reveal(5);
        assert_eq!(v.offset(), 0);
        v.reveal(299);
        assert_eq!(v.offset(), 270);
    }

    #[test]
    fn titles_join_only_what_is_there() {
        let mut v = view(120, 20, true);
        assert_eq!(
            pane_title(&["Transcript", &position_label(&v, true)]),
            " Transcript · 100% "
        );
        v.up(50);
        assert_eq!(
            pane_title(&[" Transcript ", &position_label(&v, true), ""]),
            " Transcript · 50% · End follows "
        );
        let short = view(5, 20, true);
        assert_eq!(
            pane_title(&["Summary", &position_label(&short, false)]),
            " Summary "
        );
    }

    #[test]
    fn percent_reports_position() {
        let mut v = view(120, 20, false);
        assert_eq!(v.percent(), Some(0));
        v.down(50);
        assert_eq!(v.percent(), Some(50));
        v.end();
        assert_eq!(v.percent(), Some(100));
    }

    #[test]
    fn line_rows_match_ratatui_word_wrap() {
        assert_eq!(line_rows(&Line::from(""), 10), 1);
        assert_eq!(line_rows(&Line::from("hello"), 10), 1);
        assert_eq!(line_rows(&Line::from("aaaa bbbb cccc"), 9), 2);
        // word wrap: "aa " / "bbbbb " / "cc"; dividing 11 chars by 6 would say 2 (the old
        // live pane's estimate, which left the last row of a long transcript off screen)
        assert_eq!(line_rows(&Line::from("aa bbbbb cc"), 6), 3);
        // Cyrillic is one column per char
        assert_eq!(line_rows(&Line::from("привет мир"), 6), 2);
    }

    fn lines(texts: &[&'static str]) -> Vec<&'static str> {
        texts.to_vec()
    }

    #[test]
    fn wrapped_rows_append_only_costs_new_lines_and_resets_on_resize() {
        let items = lines(&["one", "two words here", "three"]);
        let mut rows = WrappedRows::default();
        rows.sync(8, &items, |_, s| Line::from(*s));
        // "two words here" at width 8: "two " / "words " / "here" = 3 rows
        assert_eq!(rows.total(), 5);
        assert_eq!(rows.row_of(1), 1);
        assert_eq!(rows.row_of(2), 4);
        let more = lines(&["one", "two words here", "three", "four"]);
        rows.sync(8, &more, |_, s| Line::from(*s));
        assert_eq!(rows.total(), 6);
        rows.sync(80, &more, |_, s| Line::from(*s));
        assert_eq!(rows.total(), 4);
        // a shorter list (recording restarted) is recounted from scratch
        let fresh = lines(&["x"]);
        rows.sync(80, &fresh, |_, s| Line::from(*s));
        assert_eq!(rows.total(), 1);
    }

    fn draw(pane: &mut Pane, items: &[String], tail: Option<&str>) -> Vec<String> {
        use ratatui::{Terminal, backend::TestBackend};
        let mut term = Terminal::new(TestBackend::new(6, 6)).unwrap();
        term.draw(|f| {
            let tail = tail.map(Line::from);
            render_pane(
                f,
                f.area(),
                Block::bordered(),
                pane,
                items,
                |_, s| Line::from(s.as_str()),
                tail,
            )
        })
        .unwrap();
        let buf = term.backend().buffer();
        (1..5)
            .map(|y| {
                (1..5)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn render_pane_starts_a_partly_hidden_first_line_at_the_right_row() {
        // inner 4x4; each item wraps to 3 rows: "a0 " / "b0 " / "c0"
        let items: Vec<String> = (0..10).map(|i| format!("a{i} b{i} c{i}")).collect();
        let mut pane = Pane::following();
        assert_eq!(draw(&mut pane, &items, None), ["c8", "a9", "b9", "c9"]);
        pane.view.up(2);
        assert_eq!(draw(&mut pane, &items, None), ["a8", "b8", "c8", "a9"]);
        pane.view.end();
        assert_eq!(
            draw(&mut pane, &items, Some("zz")),
            ["a9", "b9", "c9", "zz"]
        );
        // only the tail is left on screen
        let mut short = Pane::following();
        let one = vec!["x".to_string()];
        let long_tail = "t1 t2 t3 t4 t5 t6";
        assert_eq!(
            draw(&mut short, &one, Some(long_tail)),
            ["t3", "t4", "t5", "t6"]
        );
    }

    #[test]
    fn window_returns_the_visible_logical_lines_and_the_hidden_rows_of_the_first() {
        // rows per line: 1, 3, 1, 2  → cum 0,1,4,5,7
        let items = lines(&["a", "aaa bbb ccc", "b", "ddd eee"]);
        let mut rows = WrappedRows::default();
        rows.sync(4, &items, |_, s| Line::from(*s));
        assert_eq!(rows.total(), 7);
        assert_eq!(rows.window(0, 2), (0, 2, 0));
        assert_eq!(rows.window(2, 2), (1, 2, 1)); // rows 2-3 are both inside line 1
        assert_eq!(rows.window(3, 3), (1, 4, 2));
        assert_eq!(rows.window(5, 10), (3, 4, 0));
        assert_eq!(WrappedRows::default().window(0, 5), (0, 0, 0));
    }
}
