#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;
mod store;
mod theme;
mod worker;

use eframe::egui;
use hots_core::{Draft, DraftPlayer};
use settings::Settings;
use worker::{Report, Worker};

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hots_app=info,hots_core=info".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "HotS Draft Helper",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

struct Recap {
    map: String,
    /// `None` when nothing here knows which side was yours.
    won: Option<bool>,
}

/// A note edited this frame. `save` marks the ones that have been finished with, since a
/// note is written a letter at a time and the server should hear about it once.
struct Edit {
    battletag: String,
    note: hots_core::PlayerNote,
    save: bool,
}

struct App {
    settings: Settings,
    login: Option<Login>,
    worker: Option<Worker>,
    draft: Option<Draft>,
    /// Set when the match of the shown draft has been played out, which turns the same
    /// cards from a thing to read into a thing to write on.
    recap: Option<Recap>,
    temp: Option<String>,
    replay_dirs: usize,
    matches: u32,
    store: String,
    progress: Option<(u32, u32, u32)>,
    errors: Vec<String>,
    editing: bool,
    sort_by_winrate: bool,
    /// Below this a winrate is noise, so it is shown grey and without a number.
    min_games: u32,
}

impl App {
    fn new() -> App {
        let settings = Settings::load();
        App {
            login: settings.needs_login().then(|| Login::of(&settings)),
            worker: (!settings.needs_login()).then(|| Worker::start(settings.clone())),
            editing: settings.battletag.is_empty(),
            min_games: settings.folders().min_games_for_winrate,
            settings,
            draft: None,
            recap: None,
            temp: None,
            replay_dirs: 0,
            matches: 0,
            store: String::new(),
            progress: None,
            errors: Vec::new(),
            sort_by_winrate: false,
        }
    }

    fn drain(&mut self) {
        let reports: Vec<Report> = match &self.worker {
            Some(worker) => worker.reports.try_iter().collect(),
            None => Vec::new(),
        };
        for report in reports {
            match report {
                Report::Store(store) => self.store = store,
                Report::Folders { temp, replays } => {
                    self.temp = temp;
                    self.replay_dirs = replays;
                }
                Report::Backfill {
                    done,
                    total,
                    failed,
                } => {
                    self.progress = (done < total).then_some((done, total, failed));
                }
                Report::Matches(count) => self.matches = count,
                Report::Lobby(draft) => {
                    self.recap = None;
                    self.draft = Some(*draft);
                }
                Report::Played {
                    battletags,
                    winners,
                    map,
                } => self.finished(&battletags, &winners, map),
                Report::Failed(e) => {
                    tracing::warn!("{e}");
                    if e.contains(store::REJECTED) {
                        self.log_out(Some(e));
                        return;
                    }
                    self.errors.push(e);
                    self.errors.truncate(20);
                }
            }
        }
    }

    /// The client leaves its battlelobby behind when the match ends, so the file alone
    /// cannot say whether a draft is still being drafted. The replay of that same draft
    /// arriving in the replay folder can, and does. The cards stay up: the minute after a
    /// game is when anyone actually has something to write about the people in it.
    fn finished(&mut self, played: &[String], winners: &[String], map: String) {
        let Some(draft) = &self.draft else {
            return;
        };
        let over = draft
            .players
            .iter()
            .all(|seat| played.iter().any(|tag| same_player(tag, &seat.battletag)));
        if !over {
            return;
        }
        let me = &self.settings.battletag;
        self.recap = Some(Recap {
            map,
            won: (!me.is_empty() && played.iter().any(|tag| same_player(tag, me)))
                .then(|| winners.iter().any(|tag| same_player(tag, me))),
        });
    }

    /// The stored name may carry no discriminator, so a name on its own still matches.
    fn stage(&self) -> (String, egui::Color32) {
        match (&self.draft, &self.recap) {
            (Some(_), Some(recap)) => {
                let how = match recap.won {
                    Some(true) => " · won",
                    Some(false) => " · lost",
                    None => "",
                };
                (format!("last game · {}{how}", recap.map), theme::DIM)
            }
            (Some(_), None) => ("in a lobby, drafting".to_string(), theme::YELLOW),
            (None, _) => ("searching for a game".to_string(), theme::BLUE),
        }
    }

    fn restart(&mut self) {
        let _ = self.settings.save();
        self.min_games = self.settings.folders().min_games_for_winrate;
        self.worker = Some(Worker::start(self.settings.clone()));
        self.draft = None;
    }

    fn log_out(&mut self, why: Option<String>) {
        self.settings.password.clear();
        let _ = self.settings.save();
        self.worker = None;
        self.draft = None;
        self.errors.clear();
        self.login = Some(Login {
            error: why,
            ..Login::of(&self.settings)
        });
    }
}

/// The shared server answers nothing without a login, so the app asks for one before it starts.
struct Login {
    server: String,
    username: String,
    password: String,
    asking: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    error: Option<String>,
}

impl Login {
    fn of(settings: &Settings) -> Login {
        Login {
            server: settings.server.clone().unwrap_or_default(),
            username: settings.username.clone(),
            password: String::new(),
            asking: None,
            error: None,
        }
    }

    /// One request the login screen waits on, off the thread that paints it.
    fn ask(settings: Settings) -> std::sync::mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let tried = store::Store::open(&settings)
                .map_err(|e| e.to_string())
                .and_then(|store| store.count())
                .map(drop);
            let _ = tx.send(tried);
        });
        rx
    }
}

impl App {
    fn login_screen(&mut self, ui: &mut egui::Ui) {
        let Some(login) = &mut self.login else {
            return;
        };

        let mut submit = false;
        egui::CentralPanel::default()
            .frame(theme::central(ui.style()))
            .show(ui, |ui| {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("DRAFT HELPER")
                            .size(22.0)
                            .strong()
                            .color(theme::YELLOW),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("the shared database asks who you are")
                            .color(theme::DIM),
                    );
                    ui.add_space(22.0);
                });

                // A form centred as one block of a fixed width, rather than each field
                // centred on its own and sized to whatever it happens to hold.
                form(ui, 400.0, |ui| {
                    egui::Grid::new("login-grid")
                        .num_columns(2)
                        .spacing([14.0, 12.0])
                        .min_col_width(76.0)
                        .show(ui, |ui| {
                            ui.label("server");
                            ui.add(
                                egui::TextEdit::singleline(&mut login.server)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.end_row();

                            ui.label("username");
                            submit |= entered(
                                ui.add(
                                    egui::TextEdit::singleline(&mut login.username)
                                        .desired_width(f32::INFINITY),
                                ),
                            );
                            ui.end_row();

                            ui.label("password");
                            submit |= entered(
                                ui.add(
                                    egui::TextEdit::singleline(&mut login.password)
                                        .password(true)
                                        .desired_width(f32::INFINITY),
                                ),
                            );
                            ui.end_row();
                        });

                    ui.add_space(18.0);
                    match login.asking.is_some() {
                        true => {
                            ui.add(egui::Spinner::new().size(20.0).color(theme::YELLOW));
                        }
                        false => {
                            let wide = egui::vec2(ui.available_width(), 32.0);
                            submit |= ui.add_sized(wide, egui::Button::new("log in")).clicked()
                        }
                    }

                    if let Some(error) = &login.error {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(error).color(theme::RED));
                    }
                });
            });

        if submit && login.asking.is_none() {
            login.error = None;
            self.settings.server = Some(login.server.clone());
            self.settings.username = login.username.clone();
            self.settings.password = login.password.clone();
            login.asking = Some(Login::ask(self.settings.clone()));
        }

        let answer = login
            .asking
            .as_ref()
            .and_then(|asking| asking.try_recv().ok());
        match answer {
            Some(Ok(())) => {
                self.login = None;
                self.restart();
            }
            Some(Err(e)) => {
                login.asking = None;
                login.error = Some(e);
            }
            None => {}
        }
    }
}

/// One block of a fixed width, centred in whatever room the window has.
fn form(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let width = width.min(ui.available_width());
    let margin = ((ui.available_width() - width) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            // Both: `set_width` is a floor, and a field asking for infinity ignores it.
            ui.set_width(width);
            ui.set_max_width(width);
            add(ui);
        });
    });
}

/// Enter in a field is the same as the button, which is what a login screen owes anyone.
fn entered(field: egui::Response) -> bool {
    field.lost_focus() && field.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));

        if self.login.is_some() {
            self.login_screen(ui);
            return;
        }

        egui::Panel::top("top")
            .frame(theme::panel(ui.style()))
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("DRAFT HELPER")
                            .size(17.0)
                            .strong()
                            .color(theme::YELLOW),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} matches", self.matches)).color(theme::DIM),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("settings").clicked() {
                            self.editing = !self.editing;
                        }
                        ui.checkbox(&mut self.sort_by_winrate, "sort by winrate");
                    });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (stage, color) = self.stage();
                    light(ui, color);
                    ui.label(egui::RichText::new(stage).strong().color(color));
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let (label, color) = match &self.temp {
                        Some(path) => (path.as_str(), theme::GREEN),
                        None => ("no temp folder found, set it in settings", theme::RED),
                    };
                    light(ui, color);
                    // The path is longer than the window on every wine install there is.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(label)
                                .size(12.0)
                                .monospace()
                                .color(theme::DIM),
                        )
                        .truncate(),
                    );
                });

                self.progress_bar(ui);
                ui.add_space(6.0);
            });

        if self.editing {
            self.settings_panel(ui);
        }
        self.errors_panel(ui);

        // Lifted out so a note typed into a card can be written back into it, which a
        // borrow of `self.draft` held across the whole panel would not allow.
        let draft = self.draft.take();
        let mut edits: Vec<Edit> = Vec::new();
        egui::CentralPanel::default()
            .frame(theme::central(ui.style()))
            .show(ui, |ui| match &draft {
                Some(shown) => self.draw_draft(ui, shown, &mut edits),
                None => self.draw_searching(ui),
            });
        self.draft = draft;
        self.apply(edits);
    }
}

impl App {
    fn draw_searching(&self, ui: &mut egui::Ui) {
        ui.add_space(70.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new().size(28.0).color(theme::YELLOW));
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("Searching for a game")
                    .size(16.0)
                    .color(theme::TEXT),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("the enemy pools appear here the moment a draft opens")
                    .color(theme::DIM),
            );
        });
    }

    /// The window edits its own copy so typing stays instant, and the store hears about a
    /// note once, when whoever wrote it has moved on from the field.
    fn apply(&mut self, edits: Vec<Edit>) {
        for edit in edits {
            if let Some(draft) = &mut self.draft {
                for seat in draft.players.iter_mut() {
                    if seat.battletag == edit.battletag {
                        seat.note = edit.note.clone();
                    }
                }
            }
            if edit.save
                && let Some(worker) = &self.worker
            {
                worker.send(worker::Command::SaveNote {
                    battletag: edit.battletag,
                    note: edit.note,
                });
            }
        }
    }
}

impl App {
    /// A long backfill is the one wait this program asks anyone to sit through.
    fn progress_bar(&self, ui: &mut egui::Ui) {
        let Some((done, total, failed)) = self.progress else {
            return;
        };
        let text = match failed {
            0 => format!("parsing replays {done}/{total}"),
            _ => format!("parsing replays {done}/{total}, {failed} unreadable"),
        };
        ui.add(
            egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                .desired_height(14.0)
                .fill(theme::BLUE)
                .corner_radius(4)
                .animate(true)
                .text(egui::RichText::new(text).color(theme::BG).strong()),
        );
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let mut log_out = false;
        egui::Panel::top("settings")
            .frame(theme::panel(ui.style()))
            .show(ui, |ui| {
                ui.add_space(4.0);
                // Two columns, every row: a label and one control. A row that wanted a
                // third cell is what pulled the other rows out of line.
                egui::Grid::new("settings-grid")
                    .num_columns(2)
                    .spacing([16.0, 12.0])
                    .min_col_width(110.0)
                    .show(ui, |ui| {
                        ui.label("your battletag");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.battletag)
                                .hint_text("Name#1234")
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label("shared server");
                        ui.vertical(|ui| {
                            let mut server = self.settings.server.clone().unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut server)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed()
                            {
                                self.settings.server = Some(server);
                            }
                            hint(ui, "empty to keep the database on this machine");
                        });
                        ui.end_row();

                        ui.label("logged in as");
                        ui.horizontal(|ui| {
                            let who = match self.settings.username.is_empty() {
                                true => "nobody",
                                false => self.settings.username.as_str(),
                            };
                            ui.label(egui::RichText::new(who).color(theme::TEXT));
                            log_out = ui.button("log out").clicked();
                        });
                        ui.end_row();

                        ui.label("replay folder");
                        ui.vertical(|ui| {
                            folder_row(ui, &mut self.settings.replay_dir);
                            hint(ui, &format!("{} found automatically", self.replay_dirs));
                        });
                        ui.end_row();

                        ui.label("temp folder");
                        ui.vertical(|ui| {
                            folder_row(ui, &mut self.settings.temp_dir);
                            hint(ui, "empty to look for it on every launch");
                        });
                        ui.end_row();
                    });

                ui.add_space(14.0);
                if ui.button("save and restart the watcher").clicked() {
                    self.restart();
                    self.editing = false;
                }
                ui.add_space(10.0);
                path_line(ui, "settings", &Settings::path().display().to_string());
                path_line(ui, "database", &self.store);
                ui.add_space(4.0);
            });
        if log_out {
            self.log_out(None);
        }
    }

    fn errors_panel(&mut self, ui: &mut egui::Ui) {
        if self.errors.is_empty() {
            return;
        }
        egui::Panel::bottom("errors")
            .frame(theme::panel(ui.style()))
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} problems", self.errors.len()))
                            .color(theme::RED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("clear").clicked() {
                            self.errors.clear();
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .max_height(90.0)
                    .show(ui, |ui| {
                        for line in self.errors.iter().rev() {
                            ui.label(egui::RichText::new(line).monospace().color(theme::RED));
                        }
                    });
            });
    }

    fn draw_draft(&self, ui: &mut egui::Ui, draft: &Draft, edits: &mut Vec<Edit>) {
        let shown: Vec<&DraftPlayer> = match draft.my_team {
            Some(_) => draft.enemies().collect(),
            None => draft.players.iter().collect(),
        };
        if draft.my_team.is_none() {
            ui.label(
                egui::RichText::new(
                    "Your battletag is not in this lobby, so every player is shown.",
                )
                .color(theme::YELLOW),
            );
            ui.add_space(8.0);
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let (columns, card) = grid_of(ui.available_width(), shown.len());
            // Rows rather than a wrap, so the cards line up down the window as well as
            // across it, whatever the widest battletag in each of them turns out to be.
            for row in shown.chunks(columns) {
                ui.horizontal_top(|ui| {
                    for player in row {
                        // The layout has to be named: a child of a horizontal parent
                        // inherits it, and the card would lay itself out sideways.
                        ui.allocate_ui_with_layout(
                            egui::vec2(card, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| self.draw_player(ui, player, edits),
                        );
                    }
                });
                ui.add_space(2.0);
            }
        });
    }

    fn draw_player(&self, ui: &mut egui::Ui, player: &DraftPlayer, edits: &mut Vec<Edit>) {
        egui::Frame::group(ui.style())
            .fill(theme::PANEL)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(6)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    // Truncated, because a long battletag would otherwise push the game
                    // count off the edge of its own card.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&player.battletag)
                                .strong()
                                .size(15.0)
                                .color(theme::TEXT),
                        )
                        .truncate(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(games(player.games)).color(theme::DIM));
                    });
                });
                ui.separator();

                if player.heroes.is_empty() {
                    ui.label(
                        egui::RichText::new("no games on record")
                            .italics()
                            .color(theme::DIM),
                    );
                } else {
                    let mut heroes: Vec<_> = player.heroes.iter().collect();
                    if self.sort_by_winrate {
                        heroes.sort_by(|a, b| {
                            b.winrate()
                                .partial_cmp(&a.winrate())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }

                    let most = heroes.iter().map(|h| h.games).max().unwrap_or(1).max(1);
                    for hero in heroes {
                        draw_hero(ui, hero, most, self.min_games);
                    }
                }

                draw_note(ui, player, edits);
            });
    }
}

/// A verdict and a line about a player, shared by everyone behind the one login. What is
/// typed shows at once; what is typed and then left behind is what gets sent.
fn draw_note(ui: &mut egui::Ui, player: &DraftPlayer, edits: &mut Vec<Edit>) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for (verdict, glyph, color) in [(1i8, UP, theme::GREEN), (-1, DOWN, theme::RED)] {
            let held = player.note.verdict == verdict;
            let text = egui::RichText::new(glyph)
                .size(15.0)
                .color(match held {
                    true => color,
                    false => theme::DIM,
                });
            let button = egui::Button::new(text)
                .min_size(egui::vec2(38.0, 26.0))
                .fill(match held {
                    true => color.gamma_multiply(0.22),
                    false => theme::RAISE,
                });
            if ui.add(button).clicked() {
                edits.push(Edit {
                    battletag: player.battletag.clone(),
                    // Clicking the verdict already held takes it back.
                    note: hots_core::PlayerNote {
                        verdict: if held { 0 } else { verdict },
                        ..player.note.clone()
                    },
                    save: true,
                });
            }
        }
    });

    ui.add_space(6.0);
    let mut text = player.note.note.clone();
    let field = ui.add(
        egui::TextEdit::multiline(&mut text)
            .desired_rows(2)
            .hint_text("note")
            .desired_width(f32::INFINITY),
    );
    if field.changed() || field.lost_focus() {
        edits.push(Edit {
            battletag: player.battletag.clone(),
            note: hots_core::PlayerNote {
                note: text,
                ..player.note.clone()
            },
            save: field.lost_focus(),
        });
    }
}

/// The bar is the games, its colour the winrate, so a pool reads without being read.
/// Painted rather than assembled, because a padded string in a progress bar is only ever
/// the right width for one card and spills off the edge of every other.
fn draw_hero(ui: &mut egui::Ui, hero: &hots_core::HeroRow, most: u32, min_games: u32) {
    let rate = hero.winrate().unwrap_or(0.0);
    let color = theme::winrate_color(hero.games, rate, min_games);
    let rate_text = match hero.winrate() {
        Some(rate) if hero.games >= min_games => format!("{:.0}%", rate * 100.0),
        _ => "-".to_string(),
    };
    let tail = format!("{:>4}  {:>4}", hero.games, rate_text);

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 20.0),
        egui::Sense::hover(),
    );
    let radius = egui::CornerRadius::same(3);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, theme::BG);
    let share = (hero.games as f32 / most.max(1) as f32).clamp(0.0, 1.0);
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * share, rect.height())),
        radius,
        color.gamma_multiply(0.30),
    );

    let font = egui::FontId::monospace(13.0);
    let numbers = painter.layout_no_wrap(tail.clone(), font.clone(), color);
    let room = rect.width() - numbers.size().x - 22.0;
    painter.text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        elide(&hero.hero, (room / 8.0).max(4.0) as usize),
        font.clone(),
        color,
    );
    painter.text(
        rect.right_center() - egui::vec2(8.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        tail,
        font,
        color,
    );
}

/// "1 games" reads as a bug in the counting.
fn games(n: u32) -> String {
    match n {
        1 => "1 game".to_string(),
        n => format!("{n} games"),
    }
}

const UP: &str = "\u{1F44D}";
const DOWN: &str = "\u{1F44E}";

/// The stored name may carry no discriminator, so a name on its own still matches.
fn same_player(a: &str, b: &str) -> bool {
    let short = |tag: &str| tag.split_once('#').map_or(tag, |(n, _)| n).to_lowercase();
    a.eq_ignore_ascii_case(b) || short(a) == short(b)
}

/// The status light. A filled circle is not in every font this runs against, and the
/// fallback box reads as an unticked checkbox, which is the one thing it is not.
fn light(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// How many cards fit across, and how wide each one is once they share the row evenly.
/// A card narrower than this stops being readable, so the count falls before the width does.
const CARD_MIN: f32 = 260.0;
const CARD_MAX: f32 = 420.0;

fn grid_of(width: f32, count: usize) -> (usize, f32) {
    if count == 0 {
        return (1, width);
    }
    let gap = 10.0;
    let fits = ((width + gap) / (CARD_MIN + gap)).floor().max(1.0) as usize;
    let columns = fits.min(count);
    let card = ((width - gap * (columns - 1) as f32) / columns as f32).min(CARD_MAX);
    (columns, card.max(CARD_MIN.min(width)))
}

fn elide(text: &str, width: usize) -> String {
    match text.chars().count() > width {
        true => format!("{}…", text.chars().take(width - 1).collect::<String>()),
        false => text.to_string(),
    }
}

fn folder_row(ui: &mut egui::Ui, dir: &mut Option<std::path::PathBuf>) {
    let mut text = dir
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let field = egui::TextEdit::singleline(&mut text)
        .hint_text("found automatically")
        .desired_width(f32::INFINITY);
    if ui.add(field).changed() {
        *dir = (!text.is_empty()).then(|| std::path::PathBuf::from(&text));
    }
}

/// The sentence under a field, which belongs to the field and not to a column of its own.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .color(theme::DIM.gamma_multiply(0.85)),
    );
}

/// A path is long and nobody reads it twice, so it stays on one line and gets cut there.
fn path_line(ui: &mut egui::Ui, label: &str, path: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [72.0, 16.0],
            egui::Label::new(egui::RichText::new(label).size(12.0).color(theme::DIM))
                .selectable(false),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(path)
                    .size(12.0)
                    .monospace()
                    .color(theme::DIM),
            )
            .truncate(),
        );
    });
}
