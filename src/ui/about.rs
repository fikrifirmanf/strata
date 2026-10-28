//! About window and update checks: Help → Check for Updates, plus a quiet check at launch that
//! only speaks up when a newer release exists.

use super::{dim, faint, p, primary_button, secondary_button};
use crate::model::Settings;
use crate::runtime::Task;
use crate::update::{self, Progress, Release};
use egui::RichText;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Default)]
enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Failed(String),
}

#[derive(Default)]
pub struct Updates {
    pub about_open: bool,
    status: Status,
    task: Option<Task<Result<Option<Release>, String>>>,
    /// Asked for from a menu: report "up to date" and errors, and ignore a skipped version.
    manual: bool,
    popup: Option<Release>,
    install: Option<(Task<Result<(), String>>, Arc<Progress>)>,
    install_error: Option<String>,
    /// Set once the update is staged; the app quits and the helper relaunches the new version.
    pub restart: bool,
}

impl Updates {
    pub fn check(&mut self, ctx: &egui::Context, manual: bool) {
        if self.task.is_some() {
            self.manual |= manual;
            return;
        }
        self.manual = manual;
        self.status = Status::Checking;
        self.task = Some(Task::spawn(ctx, async { update::check().await.map_err(|e| format!("{e:#}")) }));
    }

    /// Shows the About window, the update popup, and any finished check. Returns true when
    /// `settings` changed and should be saved.
    pub fn show(&mut self, ctx: &egui::Context, settings: &mut Settings, toasts: &mut super::Toasts) -> bool {
        let mut changed = false;
        if let Some(result) = crate::runtime::take(&mut self.task) {
            self.status = match result {
                Ok(Some(release)) => {
                    if self.manual || release.version != settings.skipped_update {
                        self.popup = Some(release.clone());
                    }
                    Status::Available(release)
                }
                Ok(None) => {
                    if self.manual && !self.about_open {
                        toasts.success(format!("Strata {} is the latest version", update::CURRENT));
                    }
                    Status::UpToDate
                }
                Err(e) => {
                    if self.manual && !self.about_open {
                        toasts.error(format!("Couldn't check for updates: {e}"));
                    }
                    Status::Failed(e)
                }
            };
        }
        if let Some(result) = self.install.as_mut().and_then(|(task, _)| task.poll()) {
            self.install = None;
            match result {
                Ok(()) => self.restart = true,
                Err(e) => self.install_error = Some(e),
            }
        }
        if self.about_open {
            changed |= self.about(ctx, settings);
        }
        if let Some(release) = self.popup.clone() {
            changed |= self.update_popup(ctx, &release, settings);
        }
        changed
    }

    fn about(&mut self, ctx: &egui::Context, settings: &mut Settings) -> bool {
        let pal = p();
        let mut changed = false;
        let mut check = false;
        let mut open_popup = None;
        self.about_open = super::dialogs::modal_window(ctx, "About Strata", 420.0, |ui| {
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                super::home::paint_logo(ui.painter(), r);
                ui.add_space(6.0);
                ui.label(RichText::new("Strata").size(22.0).strong());
                ui.label(dim(format!("Version {}", update::CURRENT)));
                ui.add_space(6.0);
                ui.label("A native, cross-platform database workbench written in Rust.");
            });
            ui.add_space(14.0);
            super::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    match &self.status {
                        Status::Idle => { ui.label(dim("Updates haven't been checked yet")); }
                        Status::Checking => { ui.spinner(); ui.label(dim("Checking for updates…")); }
                        Status::UpToDate => { ui.label(RichText::new("You're on the latest version").color(pal.success)); }
                        Status::Available(r) => { ui.label(RichText::new(format!("Strata {} is available", r.version)).color(pal.accent).strong()); }
                        Status::Failed(e) => { ui.label(RichText::new(e).color(pal.danger)).on_hover_text(e); }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Status::Available(r) = &self.status {
                            if primary_button(ui, "Update").clicked() {
                                open_popup = Some(r.clone());
                            }
                        } else if ui.add_enabled(!matches!(self.status, Status::Checking), egui::Button::new("Check for Updates")).clicked() {
                            check = true;
                        }
                    });
                });
                if ui.checkbox(&mut settings.check_updates, "Check for updates when Strata starts").changed() {
                    changed = true;
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.hyperlink_to("GitHub", update::REPO_URL);
                ui.label(faint("·"));
                ui.hyperlink_to("Release notes", format!("{}/releases", update::REPO_URL));
                ui.label(faint("·"));
                ui.hyperlink_to("Report an issue", format!("{}/issues", update::REPO_URL));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(faint("MIT License")));
            });
        });
        if check {
            self.check(ctx, true);
        }
        if let Some(release) = open_popup {
            self.about_open = false;
            self.popup = Some(release);
        }
        changed
    }

    /// Downloads and stages the update in place, or opens the release page for copies that can't
    /// replace themselves (the portable Windows zip, development builds).
    fn start_install(&mut self, ctx: &egui::Context, release: &Release) {
        if update::install_target().is_none() || release.asset.is_none() {
            ctx.open_url(egui::OpenUrl::new_tab(&release.url));
            self.popup = None;
            return;
        }
        let progress = Arc::new(Progress::default());
        let (release, shared, repaint) = (release.clone(), progress.clone(), ctx.clone());
        self.install_error = None;
        self.install = Some((
            Task::spawn(ctx, async move { update::install(release, shared, move || repaint.request_repaint()).await.map_err(|e| format!("{e:#}")) }),
            progress,
        ));
    }

    fn update_popup(&mut self, ctx: &egui::Context, release: &Release, settings: &mut Settings) -> bool {
        let mut changed = false;
        let mut close = false;
        let mut install = false;
        let open = super::dialogs::modal_window(ctx, "Update available", 460.0, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                super::home::paint_logo(ui.painter(), r);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Strata {} is available", release.version)).size(15.0).strong());
                    let published = release.published.map(|d| format!(" · released {}", d.format("%-d %b %Y"))).unwrap_or_default();
                    ui.label(dim(format!("You have {}{published}", update::CURRENT)));
                });
            });
            if !release.notes.is_empty() {
                ui.add_space(10.0);
                super::card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| ui.label(&release.notes));
                });
            }
            ui.add_space(12.0);
            if let Some((_, progress)) = &self.install {
                let (done, total) = (progress.done.load(Ordering::Relaxed), progress.total.load(Ordering::Relaxed));
                let mb = |b: u64| b as f64 / 1_048_576.0;
                let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction.min(1.0)).desired_height(6.0));
                ui.add_space(4.0);
                let text = if total > 0 && done >= total {
                    "Installing… Strata will restart".to_owned()
                } else if total > 0 {
                    format!("Downloading… {:.1} of {:.1} MB", mb(done), mb(total))
                } else {
                    format!("Downloading… {:.1} MB", mb(done))
                };
                ui.label(dim(text));
                return;
            }
            if let Some(e) = &self.install_error {
                ui.label(RichText::new(format!("Couldn't update: {e}")).color(p().danger)).on_hover_text(e);
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                if secondary_button(ui, "Skip This Version").clicked() {
                    settings.skipped_update = release.version.clone();
                    changed = true;
                    close = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.install_error.is_some() { "Retry" } else { "Update" };
                    if primary_button(ui, label).clicked() {
                        install = true;
                    }
                    if secondary_button(ui, "Later").clicked() {
                        close = true;
                    }
                });
            });
        });
        if install {
            self.start_install(ctx, release);
        }
        // The popup stays up while the update downloads.
        if (close || !open) && self.install.is_none() {
            self.popup = None;
            self.install_error = None;
        }
        changed
    }
}
