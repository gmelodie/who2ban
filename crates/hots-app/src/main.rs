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

struct App {
    settings: Settings,
    worker: Worker,
    draft: Option<Draft>,
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
            worker: Worker::start(settings.clone()),
            editing: settings.battletag.is_empty(),
            min_games: settings.folders().min_games_for_winrate,
            settings,
            draft: None,
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
        while let Ok(report) = self.worker.reports.try_recv() {
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
                Report::Lobby(draft) => self.draft = Some(*draft),
                Report::Failed(e) => {
                    tracing::warn!("{e}");
                    self.errors.push(e);
                    self.errors.truncate(20);
                }
            }
        }
    }

    fn restart(&mut self) {
        let _ = self.settings.save();
        self.min_games = self.settings.folders().min_games_for_winrate;
        self.worker = Worker::start(self.settings.clone());
        self.draft = None;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));

        egui::Panel::top("top").show(ui, |ui| {
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

            ui.horizontal(|ui| {
                let (label, color) = match &self.temp {
                    Some(path) => (path.as_str(), theme::GREEN),
                    None => ("no temp folder found, set it in settings", theme::RED),
                };
                ui.label(egui::RichText::new("\u{25cf}").color(color));
                ui.label(egui::RichText::new(label).monospace().color(theme::DIM));
            });

            self.progress_bar(ui);
            ui.add_space(6.0);
        });

        if self.editing {
            self.settings_panel(ui);
        }
        self.errors_panel(ui);

        egui::CentralPanel::default().show(ui, |ui| match &self.draft {
            Some(draft) => self.draw_draft(ui, draft),
            None => {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(28.0).color(theme::YELLOW));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Waiting for a lobby")
                            .size(16.0)
                            .color(theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new("start a game and the enemy pools appear here")
                            .color(theme::DIM),
                    );
                });
            }
        });
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
        egui::Panel::top("settings").show(ui, |ui| {
            egui::Grid::new("settings-grid").show(ui, |ui| {
                ui.label("your battletag");
                ui.text_edit_singleline(&mut self.settings.battletag);
                ui.end_row();

                ui.label("shared server");
                let mut server = self.settings.server.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut server).changed() {
                    self.settings.server = Some(server).filter(|url| !url.is_empty());
                }
                ui.label("clear it to keep the database on this machine");
                ui.end_row();

                ui.label("replay folder");
                folder_row(ui, &mut self.settings.replay_dir);
                ui.label(format!("{} found automatically", self.replay_dirs));
                ui.end_row();

                ui.label("temp folder");
                folder_row(ui, &mut self.settings.temp_dir);
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("save and restart the watcher").clicked() {
                    self.restart();
                    self.editing = false;
                }
                ui.monospace(Settings::path().display().to_string());
                ui.label("database");
                ui.monospace(&self.store);
            });
            ui.add_space(4.0);
        });
    }

    fn errors_panel(&mut self, ui: &mut egui::Ui) {
        if self.errors.is_empty() {
            return;
        }
        egui::Panel::bottom("errors").show(ui, |ui| {
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

    fn draw_draft(&self, ui: &mut egui::Ui, draft: &Draft) {
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
        }
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for player in shown {
                    self.draw_player(ui, player);
                }
            });
        });
    }

    fn draw_player(&self, ui: &mut egui::Ui, player: &DraftPlayer) {
        egui::Frame::group(ui.style())
            .fill(theme::PANEL)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(6)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(300.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&player.battletag)
                            .strong()
                            .size(15.0)
                            .color(theme::TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} games", player.games))
                                .color(theme::DIM),
                        );
                    });
                });
                ui.separator();

                if player.heroes.is_empty() {
                    ui.label(
                        egui::RichText::new("no games on record")
                            .italics()
                            .color(theme::DIM),
                    );
                    return;
                }

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
            });
    }
}

/// The bar is the games, its colour the winrate, so a pool reads without being read.
fn draw_hero(ui: &mut egui::Ui, hero: &hots_core::HeroRow, most: u32, min_games: u32) {
    let rate = hero.winrate().unwrap_or(0.0);
    let color = theme::winrate_color(hero.games, rate, min_games);
    let rate_text = match hero.winrate() {
        Some(rate) if hero.games >= min_games => format!("{:.0}%", rate * 100.0),
        _ => "-".to_string(),
    };

    ui.add(
        egui::ProgressBar::new(hero.games as f32 / most as f32)
            .desired_height(19.0)
            .corner_radius(3)
            .fill(color.gamma_multiply(0.30))
            .text(
                egui::RichText::new(format!(
                    "{:<16} {:>3}  {:>4}",
                    elide(&hero.hero, 16),
                    hero.games,
                    rate_text
                ))
                .monospace()
                .color(color),
            ),
    );
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
    if ui.text_edit_singleline(&mut text).changed() {
        *dir = (!text.is_empty()).then(|| std::path::PathBuf::from(&text));
    }
}
