use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol, StatefulImage};
use image::DynamicImage;
use std::{io, time::Duration, collections::HashMap};
use tokio::sync::mpsc;

fn load_config_theme() -> Color {
    let default_theme = Color::Yellow;
    let home = match dirs::home_dir() {
        Some(path) => path,
        None => return default_theme,
    };
    
    let config_path = home.join(".config/rmus/rmus.conf");
    
    if let Ok(content) = std::fs::read_to_string(config_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("theme_color") {
                if let Some((_, val)) = line.split_once('=') {
                    let val = val.trim().trim_matches('"').trim_matches('\'');
                    if val.starts_with('#') && val.len() == 7 {
                        let r = u8::from_str_radix(&val[1..3], 16);
                        let g = u8::from_str_radix(&val[3..5], 16);
                        let b = u8::from_str_radix(&val[5..7], 16);
                        
                        if let (Ok(r), Ok(g), Ok(b)) = (r, g, b) {
                            return Color::Rgb(r, g, b);
                        }
                    }
                }
            }
        }
    }
    default_theme
}

fn preprocess_html(html: &str) -> (Option<String>, String) {
    let mut clean_html = html.to_string();
    let mut infobox_html = None;

    let mut remove_elements = |tag: &str, keywords: &[&str]| {
        let open_tag = format!("<{}", tag);
        let close_tag = format!("</{}>", tag);
        
        let mut search_pos = 0;
        loop {
            if let Some(pos) = clean_html[search_pos..].find(&open_tag) {
                let actual_pos = search_pos + pos;
                if let Some(tag_end) = clean_html[actual_pos..].find('>') {
                    let tag_content = clean_html[actual_pos..actual_pos+tag_end].to_string();
                    
                    let mut depth = 1;
                    let mut scan_pos = actual_pos + 1;
                    let mut found_end = false;
                    
                    while depth > 0 {
                        let next_open = clean_html[scan_pos..].find(&open_tag);
                        let next_close = clean_html[scan_pos..].find(&close_tag);
                        
                        match (next_open, next_close) {
                            (Some(o), Some(c)) => {
                                if o < c {
                                    depth += 1;
                                    scan_pos += o + 1;
                                } else {
                                    depth -= 1;
                                    scan_pos += c + close_tag.len();
                                    if depth == 0 {
                                        found_end = true;
                                        let end = scan_pos;
                                        
                                        let is_target = keywords.iter().any(|k| tag_content.contains(k));
                                        
                                        if is_target {
                                            if tag == "table" && tag_content.contains("infobox") && infobox_html.is_none() {
                                                infobox_html = Some(clean_html[actual_pos..end].to_string());
                                            }
                                            
                                            clean_html.replace_range(actual_pos..end, "");
                                            search_pos = actual_pos; 
                                        } else {
                                            search_pos = actual_pos + 1;
                                        }
                                    }
                                }
                            }
                            (None, Some(c)) => {
                                depth -= 1;
                                scan_pos += c + close_tag.len();
                                if depth == 0 {
                                    let end = scan_pos;
                                    let is_target = keywords.iter().any(|k| tag_content.contains(k));
                                    if is_target {
                                        if tag == "table" && tag_content.contains("infobox") && infobox_html.is_none() {
                                            infobox_html = Some(clean_html[actual_pos..end].to_string());
                                        }
                                        clean_html.replace_range(actual_pos..end, "");
                                        search_pos = actual_pos;
                                    } else {
                                        search_pos = actual_pos + 1;
                                    }
                                    found_end = true;
                                }
                            }
                            _ => break,
                        }
                    }
                    if !found_end { break; } 
                } else { break; }
            } else { break; }
        }
    };

    remove_elements("table", &["infobox", "sidebar", "vertical-navbox", "ambox", "metadata"]);

    remove_elements("div", &["hatnote", "shortdescription", "toc", "siteSub", "mw-empty-elt"]);

    (infobox_html, clean_html)
}

fn clean_infobox_text(raw: String) -> String {
    let mut output = String::new();
    let mut last_line_empty = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.chars().all(|c| c == '_' || c == '-' || c == '─' || c == ' ' || c == '|') { continue; }
        if trimmed.starts_with("[[") || trimmed.contains("File:") { continue; }
        if trimmed.starts_with('[') && trimmed.contains("]:") { continue; }
        
        if trimmed.is_empty() {
            if !last_line_empty { output.push('\n'); last_line_empty = true; }
        } else {
            let mut res = String::new();
            let chars: Vec<char> = trimmed.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '[' {
                    let mut j = i + 1;
                    let mut is_citation = false;
                    while j < chars.len() && j < i + 6 {
                        if chars[j] == ']' { 
                            if chars[i+1..j].iter().all(|c| c.is_numeric()) {
                                is_citation = true;
                            }
                            if &chars[i+1..j] == &['e','d','i','t'] { is_citation = true; }
                            break; 
                        }
                        j += 1;
                    }
                    if is_citation { i = j + 1; continue; }
                }
                res.push(chars[i]);
                i += 1;
            }
            output.push_str(res.trim());
            output.push('\n');
            last_line_empty = false;
        }
    }
    output.trim().to_string()
}

fn parse_content_blocks(html: &str) -> (Vec<ContentBlock>, Vec<String>, Vec<(usize, String, usize)>) {
    let mut blocks = Vec::new();
    let mut image_urls = Vec::new();
    let mut chapters = Vec::new(); 
    let mut chapter_counter = 1;

    let parts: Vec<&str> = html.split("<img").collect();
    
    let mut process_text = |text_html: &str, block_idx_offset: usize| {
        let text = html2text::from_read(text_html.as_bytes(), 10000);
        let mut clean_lines = Vec::new();
        let mut found_lines = false;

        for line in text.lines() {
            let trimmed = line.trim();
            
            if trimmed.starts_with('[') && trimmed.contains("]:") { continue; } 
            
            if trimmed.starts_with('*') && (trimmed.contains("Jump to search") || trimmed.contains("Jump to navigation")) { continue; }
            
            if !trimmed.is_empty() && trimmed.chars().all(|c| c == '=' || c == '-') { continue; }

            if trimmed.starts_with("* [") && trimmed.contains("][") { continue; }

            if trimmed.contains("redirects here") && (trimmed.contains("For other uses") || trimmed.contains("disambiguation")) { continue; }
            if trimmed.starts_with("This article is part of a series") { continue; }

            let mut is_header = false;
            let mut display_text = trimmed.to_string();

            if trimmed.starts_with('#') {
                is_header = true;
                display_text = trimmed.trim_start_matches('#').trim().to_string();
            } else if trimmed.starts_with("==") && trimmed.ends_with("==") {
                is_header = true;
                display_text = trimmed.trim_matches('=').trim().to_string();
            }

            if is_header {
                if !display_text.is_empty() && display_text != "Contents" {
                    chapters.push((chapter_counter, display_text.clone(), block_idx_offset + 1)); 
                    chapter_counter += 1;
                    clean_lines.push(format!("###HEADER###{}", display_text));
                    found_lines = true;
                    continue;
                }
            }

            if !trimmed.is_empty() {
                let mut res = String::new();
                let chars: Vec<char> = trimmed.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i] == '[' {
                        let mut j = i + 1;
                        let mut closed = false;
                        while j < chars.len() {
                            if chars[j] == ']' { closed = true; break; }
                            j += 1;
                        }
                        
                        if closed {
                            let content = &chars[i+1..j];
                            let content_str: String = content.iter().collect();
                            
                            if content_str.chars().all(|c| c.is_numeric()) || content_str == "edit" || content_str.chars().count() == 1 {
                                i = j + 1;
                                continue;
                            }
                            
                            i += 1; 
                            continue; 
                        }
                    }
                    if chars[i] == ']' {
                        i += 1;
                        continue;
                    }
                    res.push(chars[i]);
                    i += 1;
                }
                
                let final_line = res.trim();
                if !final_line.is_empty() {
                    clean_lines.push(final_line.to_string());
                    found_lines = true;
                }
            }
        }
        
        if found_lines {
            return Some(clean_lines.join("\n"));
        }
        None
    };

    if !parts.is_empty() {
        if let Some(t) = process_text(parts[0], blocks.len()) {
            blocks.push(ContentBlock::Text(t));
        }
    }

    for part in parts.iter().skip(1) {
        if let Some(tag_end) = part.find('>') {
            let tag_content = &part[..tag_end];
            let remainder = &part[tag_end+1..];
            
            let mut url = None;
            if let Some(src_idx) = tag_content.find("src=\"") {
                let s = &tag_content[src_idx + 5..];
                if let Some(q_end) = s.find('"') {
                    let u = &s[..q_end];
                    if u.contains("upload.wikimedia.org") && !u.ends_with(".svg") {
                        url = Some(if u.starts_with("//") { format!("https:{}", u) } else { u.to_string() });
                    }
                }
            }
            
            let is_big = if let Some(w_idx) = tag_content.find("width=\"") {
                 let s = &tag_content[w_idx + 7..];
                 let num_str: String = s.chars().take_while(|c| c.is_numeric()).collect();
                 num_str.parse::<u32>().unwrap_or(0) > 100
            } else { false };

            if let Some(u) = url {
                if is_big {
                    image_urls.push(u.clone());
                    blocks.push(ContentBlock::Image(u));
                }
            }

            if let Some(t) = process_text(remainder, blocks.len()) {
                blocks.push(ContentBlock::Text(t));
            }
        }
    }
    
    (blocks, image_urls, chapters)
}

#[derive(Clone, Debug)]
struct SearchResult {
    title: String,
    snippet: String,
}

#[derive(Clone, Debug)]
enum ContentBlock {
    Text(String),
    Image(String),
}

#[derive(Clone, Debug)]
enum AppState {
    Home,
    Searching, 
    Command,
    Chapters,
    Loading,
    ResultsList,
    Reading,
    Error(String),
}

struct App {
    state: AppState,
    input: String, 
    search_results: Vec<SearchResult>,
    selected_index: usize,
    theme: Color,
    
    current_article_title: String,
    current_article_info: String, 
    content_blocks: Vec<ContentBlock>,
    chapters: Vec<(usize, String, usize)>,
    
    scroll_offset: u16,
    chapter_list_state: ListState,
    
    image_picker: Picker,
    image_protocols: HashMap<String, StatefulProtocol>,
    
    action_tx: mpsc::UnboundedSender<Action>,
}

enum Action {
    Search(String),
    FetchArticle(String),
    DownloadImage(String),
}

enum NetworkEvent {
    SearchResults(Vec<SearchResult>),
    ArticleLoaded {
        title: String,
        infobox: String,
        blocks: Vec<ContentBlock>,
        images: Vec<String>,
        chapters: Vec<(usize, String, usize)>,
    },
    ArticleImageDownloaded(String, DynamicImage),
    ThemeUpdate(Color),
    Error(String),
}

async fn run_network_loop(mut action_rx: mpsc::UnboundedReceiver<Action>, event_tx: mpsc::UnboundedSender<NetworkEvent>) {
    let client = reqwest::Client::builder()
        .user_agent("WikiTui/0.1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    while let Some(action) = action_rx.recv().await {
        let client = client.clone();
        let event_tx = event_tx.clone();

        tokio::spawn(async move {
            match action {
                Action::Search(query) => {
                    let params = [("action", "opensearch"), ("search", query.as_str()), ("limit", "10"), ("namespace", "0"), ("format", "json")];
                    if let Ok(resp) = client.get("https://en.wikipedia.org/w/api.php").query(&params).send().await {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if let Some(array) = json.as_array() {
                                if array.len() >= 4 {
                                    let titles = array[1].as_array().unwrap();
                                    let urls = array[3].as_array().unwrap(); 
                                    let results: Vec<SearchResult> = titles.iter().zip(urls.iter()).map(|(t, u)| {
                                        SearchResult { title: t.as_str().unwrap_or("").to_string(), snippet: u.as_str().unwrap_or("").to_string() }
                                    }).collect();
                                    let _ = event_tx.send(NetworkEvent::SearchResults(results));
                                }
                            }
                        }
                    }
                }
                Action::FetchArticle(title) => {
                    let parse_client = client.clone();
                    let parse_tx = event_tx.clone();
                    let title_parse = title.clone();
                    
                    tokio::spawn(async move {
                        let params = [("action", "parse"), ("format", "json"), ("prop", "text"), ("page", title_parse.as_str()), ("redirects", "1")];
                        if let Ok(resp) = parse_client.get("https://en.wikipedia.org/w/api.php").query(&params).send().await {
                            if let Ok(json) = resp.json::<serde_json::Value>().await {
                                if let Some(html_val) = json.pointer("/parse/text/*") {
                                    if let Some(html) = html_val.as_str() {
                                        let (infobox_raw, clean_main_html) = preprocess_html(html);
                                        
                                        let infobox_text = if let Some(ib) = infobox_raw {
                                            let t = html2text::from_read(ib.as_bytes(), 50);
                                            clean_infobox_text(t)
                                        } else { String::new() };

                                        let (blocks, images, chapters) = parse_content_blocks(&clean_main_html);
                                        
                                        let _ = parse_tx.send(NetworkEvent::ArticleLoaded {
                                            title: title_parse,
                                            infobox: infobox_text,
                                            blocks,
                                            images,
                                            chapters,
                                        });
                                    }
                                }
                            }
                        }
                    });
                }
                Action::DownloadImage(url) => {
                    if let Ok(resp) = client.get(&url).send().await {
                        if let Ok(bytes) = resp.bytes().await {
                            if let Ok(img) = image::load_from_memory(&bytes) {
                                let _ = event_tx.send(NetworkEvent::ArticleImageDownloaded(url, img));
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn run_config_watcher(event_tx: mpsc::UnboundedSender<NetworkEvent>) {
    let mut last_color = load_config_theme();
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        interval.tick().await;
        let new_color = load_config_theme();
        if new_color != last_color {
            last_color = new_color;
            let _ = event_tx.send(NetworkEvent::ThemeUpdate(new_color));
        }
    }
}

impl App {
    fn new(action_tx: mpsc::UnboundedSender<Action>) -> Self {
        let image_picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 12)));
        Self {
            state: AppState::Home,
            input: String::new(),
            search_results: vec![],
            selected_index: 0,
            theme: load_config_theme(),
            current_article_title: String::new(),
            current_article_info: String::new(),
            content_blocks: Vec::new(),
            chapters: Vec::new(),
            scroll_offset: 0,
            chapter_list_state: ListState::default(),
            image_picker,
            image_protocols: HashMap::new(),
            action_tx,
        }
    }

    fn on_tick(&mut self, event: Option<NetworkEvent>) {
        if let Some(network_event) = event {
            match network_event {
                NetworkEvent::SearchResults(results) => {
                    self.search_results = results;
                    self.selected_index = 0;
                    self.state = AppState::ResultsList;
                }
                NetworkEvent::ArticleLoaded { title, infobox, blocks, images, chapters } => {
                    self.current_article_title = title;
                    self.current_article_info = infobox;
                    self.content_blocks = blocks;
                    self.chapters = chapters;
                    self.image_protocols.clear();
                    self.scroll_offset = 0;
                    self.state = AppState::Reading;
                    self.chapter_list_state.select(Some(0));
                    
                    for url in images {
                        let _ = self.action_tx.send(Action::DownloadImage(url));
                    }
                }
                NetworkEvent::ArticleImageDownloaded(url, img) => {
                    let protocol = self.image_picker.new_resize_protocol(img);
                    self.image_protocols.insert(url, protocol);
                }
                NetworkEvent::ThemeUpdate(new_color) => {
                    self.theme = new_color;
                }
                NetworkEvent::Error(msg) => { self.state = AppState::Error(msg); }
            }
        }
    }

    fn handle_key(&mut self, key: KeyCode) -> bool {
        match self.state {
            AppState::Reading => {
                match key {
                    KeyCode::Char('q') => return true,
                    KeyCode::Esc => { self.state = AppState::ResultsList; }
                    KeyCode::Char('/') => { self.input.clear(); self.state = AppState::Searching; }
                    KeyCode::Char(':') => { self.input.clear(); self.state = AppState::Command; }
                    KeyCode::Char('c') => { 
                        self.state = AppState::Chapters; 
                        if self.chapter_list_state.selected().is_none() && !self.chapters.is_empty() {
                            self.chapter_list_state.select(Some(0));
                        }
                    }
                    KeyCode::Char('j') | KeyCode::Down => self.scroll_offset += 1,
                    KeyCode::Char('k') | KeyCode::Up => if self.scroll_offset > 0 { self.scroll_offset -= 1 },
                    _ => {}
                }
            }
            AppState::Chapters => {
                match key {
                    KeyCode::Esc | KeyCode::Char('c') => { self.state = AppState::Reading; }
                    KeyCode::Char('q') => return true,
                    KeyCode::Char('j') | KeyCode::Down => {
                        let i = self.chapter_list_state.selected().unwrap_or(0);
                        if i < self.chapters.len().saturating_sub(1) {
                            self.chapter_list_state.select(Some(i + 1));
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let i = self.chapter_list_state.selected().unwrap_or(0);
                        if i > 0 {
                            self.chapter_list_state.select(Some(i - 1));
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(i) = self.chapter_list_state.selected() {
                            if let Some((_, _, block_idx)) = self.chapters.get(i) {
                                self.scroll_offset = (*block_idx as u16) * 10;
                            }
                        }
                        self.state = AppState::Reading;
                    }
                    _ => {}
                }
            }
            AppState::Command => {
                match key {
                    KeyCode::Esc => { self.state = AppState::Reading; self.input.clear(); }
                    KeyCode::Enter => {
                        if let Ok(idx) = self.input.parse::<usize>() {
                            if let Some((_, _, block_idx)) = self.chapters.iter().find(|(i, _, _)| *i == idx) {
                                self.scroll_offset = (*block_idx as u16) * 10; 
                            }
                        }
                        self.state = AppState::Reading;
                        self.input.clear();
                    }
                    KeyCode::Char(c) => self.input.push(c),
                    KeyCode::Backspace => { self.input.pop(); },
                    _ => {}
                }
            }
            AppState::Searching => {
                match key {
                    KeyCode::Esc => { 
                        self.state = AppState::Home; 
                        self.input.clear();
                    }
                    KeyCode::Enter => {
                        if !self.input.is_empty() {
                            self.state = AppState::Loading;
                            let _ = self.action_tx.send(Action::Search(self.input.clone()));
                        }
                    }
                    KeyCode::Backspace => { self.input.pop(); },
                    KeyCode::Char(c) => self.input.push(c),
                    _ => {}
                }
            }
            _ => {
                match key {
                    KeyCode::Char('q') => return true,
                    KeyCode::Esc => self.state = AppState::Home,
                    KeyCode::Char('/') => { self.input.clear(); self.state = AppState::Searching; }
                    KeyCode::Enter => if let AppState::ResultsList = self.state { self.select_item() },
                    KeyCode::Char('j') | KeyCode::Down => self.move_down(),
                    KeyCode::Char('k') | KeyCode::Up => self.move_up(),
                    _ => {}
                }
            }
        }
        false
    }

    fn handle_mouse(&mut self, mouse: event::MouseEvent) {
        if mouse.kind == MouseEventKind::Down(event::MouseButton::Left) {
        }
    }

    fn move_down(&mut self) {
        if let AppState::ResultsList = self.state {
            if !self.search_results.is_empty() && self.selected_index < self.search_results.len() - 1 { self.selected_index += 1; }
        }
    }

    fn move_up(&mut self) {
        if let AppState::ResultsList = self.state {
            if self.selected_index > 0 { self.selected_index -= 1; }
        }
    }

    fn select_item(&mut self) {
        if let Some(item) = self.search_results.get(self.selected_index) {
            self.state = AppState::Loading;
            let _ = self.action_tx.send(Action::FetchArticle(item.title.clone()));
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let (main_area, bottom_area) = if matches!(app.state, AppState::Home | AppState::Reading | AppState::Chapters) {
        let c = Layout::vertical([Constraint::Min(0)]).split(f.area()); (c[0], Rect::default())
    } else {
        let c = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(f.area()); (c[0], c[1])
    };

    let theme = app.theme;
    let border = move |t: &str| {
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme)).title(Span::styled(format!(" {} ", t), Style::default().fg(theme)))
    };

    match &app.state {
        AppState::Home => {
            let t = vec![
                Line::from(vec![Span::styled("Welcome to rwiki", Style::default().fg(app.theme).add_modifier(Modifier::BOLD))]),
                Line::from(""),
                Line::from("Controls"),
                Line::from("────────"),
                Line::from("  /      : Search"),
                Line::from("  Enter  : Select Article"),
                Line::from("  j / k  : Scroll"),
                Line::from("  :      : Jump to Chapter"),
                Line::from("  c      : Chapters Mode"),
                Line::from("  q      : Quit"),
            ];
            f.render_widget(Paragraph::new(t).alignment(Alignment::Center).block(border("Home")), main_area);
        }
        AppState::Searching => {
            f.render_widget(Paragraph::new(app.input.as_str()).style(Style::default().fg(app.theme)).block(border("Search Query")), bottom_area);
            f.render_widget(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::DarkGray)), main_area);
        }
        AppState::Command => {
            let cmd_text = format!(":{}", app.input);
            f.render_widget(Paragraph::new(cmd_text).style(Style::default().fg(Color::Cyan)).block(border("Command")), bottom_area);
            render_reading_view(f, app, main_area, border); 
        }
        AppState::Loading => {
            f.render_widget(Paragraph::new("Fetching...").alignment(Alignment::Center).style(Style::default().fg(app.theme).add_modifier(Modifier::RAPID_BLINK)), main_area);
        }
        AppState::ResultsList => {
            let items: Vec<ListItem> = app.search_results.iter().enumerate().map(|(i, r)| {
                let style = if i == app.selected_index { Style::default().fg(Color::Black).bg(app.theme) } else { Style::default() };
                ListItem::new(format!(" {} ", r.title)).style(style)
            }).collect();
            f.render_widget(List::new(items).block(border("Search Results")), main_area);
        }
        AppState::Reading | AppState::Chapters => {
            render_reading_view(f, app, main_area, border);
        }
        AppState::Error(msg) => {
            f.render_widget(Paragraph::new(format!("Error: {}", msg)).style(Style::default().fg(Color::Red)).block(Block::default().borders(Borders::ALL)), main_area);
        }
    }

    if matches!(app.state, AppState::Reading | AppState::Chapters) {
    } else if !matches!(app.state, AppState::Searching | AppState::Command) {
        f.render_widget(Paragraph::new(" [ /: Search ] [ q: Quit ] [ Enter: Select ] ").style(Style::default().bg(app.theme).fg(Color::Black)), bottom_area);
    }
}

fn render_reading_view<F>(f: &mut Frame, app: &mut App, area: Rect, border: F) 
where F: Fn(&str) -> Block<'static>
{
    let (content_area, side_area) = if !app.chapters.is_empty() {
        let c = Layout::horizontal([Constraint::Min(40), Constraint::Length(40)]).split(area);
        (c[0], Some(c[1]))
    } else { (area, None) };

    let inner_content = border(&app.current_article_title).inner(content_area);
    f.render_widget(border(&app.current_article_title), content_area);
    
    let content_width = inner_content.width.saturating_sub(1) as usize;
    let mut y_draw = 0;
    let mut current_scroll_row = 0;
    let max_height = inner_content.height;
    
    let mut active_image_url = None;
    let mut found_active = false;

    for (i, block) in app.content_blocks.iter().enumerate() {
        if y_draw >= max_height { break; }

        match block {
            ContentBlock::Text(raw_text) => {
                for line_str in raw_text.lines() {
                    if line_str.starts_with("###HEADER###") {
                        let header_text = &line_str[12..]; 
                        if current_scroll_row >= app.scroll_offset as usize {
                            if y_draw < max_height {
                                f.render_widget(
                                    Paragraph::new(Span::styled(header_text, Style::default().fg(app.theme).add_modifier(Modifier::BOLD))),
                                    Rect::new(inner_content.x, inner_content.y + y_draw, inner_content.width, 1)
                                );
                                y_draw += 1;
                            }
                        }
                        current_scroll_row += 1;
                        if current_scroll_row >= app.scroll_offset as usize {
                             if y_draw < max_height { y_draw += 1; }
                        }
                        current_scroll_row += 1;
                        continue;
                    }

                    let wrapped = textwrap::wrap(line_str, content_width);
                    for w in wrapped {
                        if current_scroll_row >= app.scroll_offset as usize {
                            if y_draw < max_height {
                                f.render_widget(Paragraph::new(w.into_owned()), Rect::new(inner_content.x, inner_content.y + y_draw, inner_content.width, 1));
                                y_draw += 1;
                            }
                            if !found_active && y_draw > 0 && y_draw < 15 { 
                                if i + 1 < app.content_blocks.len() {
                                    if let ContentBlock::Image(url) = &app.content_blocks[i+1] {
                                        active_image_url = Some(url.clone());
                                        found_active = true;
                                    }
                                }
                                if !found_active && i > 0 {
                                    if let ContentBlock::Image(url) = &app.content_blocks[i-1] {
                                        active_image_url = Some(url.clone());
                                        found_active = true;
                                    }
                                }
                            }
                        }
                        current_scroll_row += 1;
                    }
                }
            },
            ContentBlock::Image(_) => {}
        }
    }

    if let Some(s_area) = side_area {
        let s_chunks = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(s_area);
        
        let ctx_block = border("Context");
        let ctx_inner = ctx_block.inner(s_chunks[0]);
        f.render_widget(ctx_block, s_chunks[0]);

        if let Some(url) = active_image_url {
            if let Some(protocol) = app.image_protocols.get_mut(&url) {
                f.render_stateful_widget(StatefulImage::default(), ctx_inner, protocol);
            } else {
                f.render_widget(Paragraph::new("[Loading Image...]").alignment(Alignment::Center).style(Style::default().fg(Color::DarkGray)), ctx_inner);
            }
        } 
        
        let is_chapters_focused = matches!(app.state, AppState::Chapters);
        let chap_color = if is_chapters_focused { Color::Cyan } else { app.theme };
        let chap_block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(chap_color)).title(Span::styled(" Chapter Reference ", Style::default().fg(chap_color)));
        let chap_inner = chap_block.inner(s_chunks[1]);
        f.render_widget(chap_block, s_chunks[1]);
        
        let chap_lines: Vec<ListItem> = app.chapters.iter()
            .map(|(idx, title, _)| ListItem::new(format!("{}. {}", idx, title)))
            .collect();
        let list = List::new(chap_lines).highlight_style(Style::default().bg(app.theme).fg(Color::Black));
        f.render_stateful_widget(list, chap_inner, &mut app.chapter_list_state);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?; 
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (action_tx, action_rx) = mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    
    let mut app = App::new(action_tx);
    tokio::spawn(run_network_loop(action_rx, event_tx.clone()));
    tokio::spawn(run_config_watcher(event_tx));

    let tick_rate = Duration::from_millis(100);
    let mut last_tick = std::time::Instant::now();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        if crossterm::event::poll(tick_rate.checked_sub(last_tick.elapsed()).unwrap_or(Duration::from_secs(0)))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press { if app.handle_key(key.code) { break; } }
                },
                _ => {}
            }
        }
        while let Ok(e) = event_rx.try_recv() { app.on_tick(Some(e)); }
        if last_tick.elapsed() >= tick_rate { app.on_tick(None); last_tick = std::time::Instant::now(); }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
