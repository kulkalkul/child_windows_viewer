use std::{io, mem};
use std::ptr::addr_of_mut;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use tui::{Frame, Terminal};
use tui::backend::{Backend, CrosstermBackend};
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, List, ListItem, ListState};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, EnumThreadWindows, EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId};

fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    execute!(terminal.backend_mut(), EnterAlternateScreen)?;

    run_app(&mut terminal)?;

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

struct AppState {
    windows: StatefulList<Window>,
    children: StatefulList<Window>,
}

impl AppState {
    fn new() -> Self {
        Self {
            windows: StatefulList::from(enum_windows()),
            children: StatefulList::from(vec![]),
        }
    }
    fn select_children(&mut self) {
        if let Some(selected) = self.windows.selected_item() {
            self.children.update(enum_child_windows(selected));
        }
    }
}

struct StatefulList<T> {
    state: ListState,
    vec: Vec<T>,
}

impl<T> StatefulList<T> {
    fn update(&mut self, items: Vec<T>) {
        if self.state.selected().map(|selected| selected > items.len()).unwrap_or_default() {
            self.state.select(Some(items.len() - 1));
        }
        self.vec = items;
    }
    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => if i >= self.vec.len() - 1 { 0 } else { i + 1 }
            None => 0,
        };

        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => if i == 0 { self.vec.len() - 1 } else { i - 1 }
            None => 0,
        };

        self.state.select(Some(i));
    }
    fn selected(&self) -> Option<usize> { self.state.selected() }
    fn selected_item(&self) -> Option<&T> { self.selected().map(|i| &self.vec[i]) }
}

impl<T> From<Vec<T>> for StatefulList<T> {
    fn from(vec: Vec<T>) -> Self {
        let mut state = ListState::default();

        if !vec.is_empty() {
            state.select(Some(0));
        }

        Self {
            state,
            vec,
        }
    }
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    const RATE: Duration = Duration::from_millis(250);

    let mut app_state = AppState::new();
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &mut app_state))?;

        let timeout = RATE
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = crossterm::event::read()? {
                use KeyCode::*;

                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, Char('c')) => break,
                    (_, Char('q')) => break,
                    (_, Up) => {
                        app_state.windows.previous();
                        app_state.select_children();
                    },
                    (_, Down) => {
                        app_state.windows.next();
                        app_state.select_children();
                    },
                    (_, Char('r')) => {
                        app_state.windows.update(enum_windows());
                        app_state.select_children();
                    },
                    _ => (),
                }
            }
        }

        if last_tick.elapsed() >= RATE {
            last_tick = Instant::now();
            app_state.select_children();
        }
    }

    Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>, app_state: &mut AppState) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(f.size());

    let main_windows = create_window_list(&app_state.windows,Block::default()
        .title("Main Windows")
        .style(Style::default().fg(Color::Blue))
        .borders(Borders::ALL)
    );

    let child_windows = create_window_list(&app_state.children, Block::default()
        .title("Children of Selected")
        .style(Style::default().fg(Color::Red))
        .borders(Borders::ALL)
    );

    f.render_stateful_widget(main_windows, layout[0], &mut app_state.windows.state);
    f.render_stateful_widget(child_windows, layout[1], &mut app_state.children.state);
}

fn create_window_list<'a, 'b>(windows: &'a StatefulList<Window>, block: Block<'b>) -> List<'b> {
    let mut items = windows
        .vec
        .iter()
        .map(|window| {
            let spans = Spans::from(vec![
                Span::styled(window.class_name.clone(), Style::default().fg(Color::Magenta)),
                Span::raw("->"),
                Span::from(window.window_text.clone()),
            ]);
            ListItem::new(spans)
        })
        .collect::<Vec<ListItem>>();

    if let Some(selected) = windows.selected() {
        let selected = &mut items[selected];
        *selected = selected.clone().style(Style::default().bg(Color::DarkGray));
    }

    List::new(items)
        .block(block)
        .style(Style::default().fg(Color::White))
}

#[derive(Debug, Clone)]
struct Window {
    class_name: String,
    window_text: String,
    handle: HWND,
    process_id: u32,
}

fn enum_windows() -> Vec<Window> {
    let mut windows: Vec<Window> = Vec::new();
    let pointer = addr_of_mut!(windows) as isize;

    unsafe { EnumWindows(Some(enum_window), LPARAM(pointer)); }

    windows
        .into_iter()
        .filter(|window| match (&window.class_name, &window.window_text) {
            (class_name, _) if class_name.is_empty() => false,
            (class_name, _) if class_name.starts_with("IME") => false,
            (class_name, _) if class_name.starts_with("MSCTFIME UI") => false,
            (class_name, _) if class_name.starts_with("WindowsForms10") => false,
            (_, window_text) if window_text.is_empty() => false,
            _ => true,
        })
        .collect()
}

fn enum_child_windows(parent: &Window) -> Vec<Window> {
    let mut windows: Vec<Window> = Vec::new();
    let pointer = addr_of_mut!(windows) as isize;

    unsafe { EnumChildWindows(parent.handle, Some(enum_window), LPARAM(pointer)); }
    unsafe { EnumThreadWindows(parent.process_id, Some(enum_window), LPARAM(pointer)); }

    windows
}

unsafe extern "system" fn enum_window(handle: HWND, windows_pointer: LPARAM) -> BOOL {
    let windows: &mut Vec<Window> = mem::transmute(windows_pointer.0 as *const u8);

    let mut text: [u16; 512] = [0; 512];
    let len = GetClassNameW(handle, &mut text);
    let class_name = String::from_utf16_lossy(&text[..len as usize]);

    let len = GetWindowTextW(handle, &mut text);
    let window_text = String::from_utf16_lossy(&text[..len as usize]);

    let process_id = GetWindowThreadProcessId(handle, None);

    windows.push(Window {
        class_name,
        window_text,
        handle,
        process_id,
    });

    true.into()
}