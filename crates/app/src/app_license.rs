//! 설정 → 라이선스: register the key bought on the website for this computer, see what it covers,
//! and free the computer again. Network calls run on a background thread; the registered license is
//! checked offline at start, and with the server about once a week (a refunded license goes away).

use crate::app::PhotoApp;
use crate::license::{self, License, LicenseConfig, LicenseError};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, vec2, Rect, Sense, Ui};

enum Done {
    Activated(Result<License, LicenseError>),
    Refreshed(Result<License, LicenseError>),
    Deactivated(Result<(), LicenseError>),
}

#[derive(Default)]
pub struct LicenseState {
    /// `None`: licensing is not set up in this build (the section is hidden).
    pub config: Option<LicenseConfig>,
    pub current: Option<License>,
    pub input: String,
    /// The last outcome to show: (is an error, text).
    pub message: Option<(bool, String)>,
    job: Option<Receiver<Done>>,
}

/// `2027-11-05` → `2027년 11월 5일`
fn korean_date(ymd: &str) -> String {
    let p: Vec<u32> = ymd.split('-').filter_map(|s| s.parse().ok()).collect();
    if p.len() == 3 {
        format!("{}년 {}월 {}일", p[0], p[1], p[2])
    } else {
        ymd.to_string()
    }
}

impl PhotoApp {
    /// At start: the license registered here (checked offline), and now and then the server.
    pub fn license_init(&mut self, ctx: &egui::Context) {
        let Some(cfg) = LicenseConfig::get() else { return };
        self.license.current = license::load(&cfg);
        self.license.config = Some(cfg);
        let due = self.license.current.as_ref().is_some_and(|l| l.due_for_check(license::now()));
        if due {
            let lic = self.license.current.clone().expect("checked above");
            self.license_spawn(ctx, move |cfg| Done::Refreshed(license::refresh(cfg, &lic)));
        }
    }

    fn license_spawn(&mut self, ctx: &egui::Context, work: impl FnOnce(&LicenseConfig) -> Done + Send + 'static) {
        let Some(cfg) = self.license.config.clone() else { return };
        let (tx, rx) = unbounded();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("license".into())
            .spawn(move || {
                let _ = tx.send(work(&cfg));
                ctx.request_repaint();
            })
            .expect("spawn license");
        self.license.job = Some(rx);
    }

    fn license_activate(&mut self, ctx: &egui::Context) {
        let key = self.license.input.clone();
        self.license.message = None;
        self.license_spawn(ctx, move |cfg| Done::Activated(license::activate(cfg, &key)));
    }

    fn license_deactivate(&mut self, ctx: &egui::Context) {
        let Some(lic) = self.license.current.clone() else { return };
        self.license.message = None;
        self.license_spawn(ctx, move |cfg| Done::Deactivated(license::deactivate(cfg, &lic)));
    }

    /// Called every frame from `poll`.
    pub fn poll_license(&mut self) {
        let Some(rx) = &self.license.job else { return };
        let Ok(done) = rx.try_recv() else { return };
        self.license.job = None;
        match done {
            Done::Activated(Ok(lic)) => {
                self.license.current = Some(lic);
                self.license.input.clear();
                self.license.message = Some((false, "이 컴퓨터에 등록했어요. 고맙습니다!".into()));
                self.toast("라이선스를 등록했어요", false);
            }
            Done::Activated(Err(e)) => self.license.message = Some((true, e.to_string())),
            Done::Refreshed(Ok(lic)) => self.license.current = Some(lic),
            Done::Refreshed(Err(e)) if e.is_revoked() => {
                self.license.current = None;
                self.license.message = Some((true, "라이선스가 해지되어(환불 등) 이 컴퓨터에서 빠졌어요.".into()));
            }
            // Offline or the server is down: the license stays — it works without a connection.
            Done::Refreshed(Err(e)) => log::info!("license check: {e}"),
            Done::Deactivated(Ok(())) => {
                self.license.current = None;
                self.license.message =
                    Some((false, "이 컴퓨터에서 해제했어요. 다른 컴퓨터에 등록할 수 있어요.".into()));
            }
            Done::Deactivated(Err(e)) => self.license.message = Some((true, e.to_string())),
        }
    }

    /// The settings section (nothing while licensing is not set up in this build).
    pub fn license_settings(&mut self, ui: &mut Ui) {
        let Some(cfg) = self.license.config.clone() else { return };
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let busy = self.license.job.is_some();
        ui.label(egui::RichText::new("라이선스").font(bold(13.0)).color(pal.text2));

        if let Some(lic) = self.license.current.clone() {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 8.0, pal.success);
                widgets::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(rect.center(), vec2(10.0, 10.0)),
                    Icon::Check,
                    pal.panel_bg,
                );
                ui.label(egui::RichText::new("이 컴퓨터에 등록됨").font(bold(13.5)));
            });
            ui.label(egui::RichText::new(format!("{}  ·  {}", lic.claims.email, lic.masked_key())).font(font(12.5)));
            ui.label(
                egui::RichText::new(format!(
                    "새 버전 무료 {}까지 · 컴퓨터 {}대까지",
                    korean_date(&lic.claims.upd),
                    lic.claims.max
                ))
                .font(font(12.0))
                .color(pal.text2),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let label = if busy { "해제하는 중…" } else { "이 컴퓨터에서 해제" };
                if widgets::secondary_button(ui, label, &pal).clicked() && !busy {
                    self.license_deactivate(&ctx);
                }
                if widgets::secondary_button(ui, "웹에서 관리…", &pal).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(cfg.url("/account")));
                }
            });
        } else {
            ui.label(
                egui::RichText::new(
                    "구매한 라이선스 키를 넣으면 이 컴퓨터가 등록돼요. 키는 웹사이트의 ‘내 라이선스’에 있어요.",
                )
                .font(font(11.5))
                .color(pal.text3),
            );
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.license.input)
                        .id_salt("license-key")
                        .hint_text("PO-XXXXX-XXXXX-XXXXX-XXXXX")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(290.0),
                );
                let valid = license::normalize_key(&self.license.input).is_some();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let label = if busy { "등록하는 중…" } else { "등록" };
                if (widgets::primary_button(ui, label, &pal, valid && !busy).clicked() || enter) && valid && !busy {
                    self.license_activate(&ctx);
                }
            });
            if widgets::secondary_button(ui, "라이선스 구매…", &pal).clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(cfg.url("/checkout")));
            }
        }
        if let Some((error, text)) = &self.license.message {
            ui.label(egui::RichText::new(text).font(font(12.0)).color(if *error { pal.danger } else { pal.success }));
        }
        ui.add_space(12.0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_read_in_korean() {
        assert_eq!(super::korean_date("2027-11-05"), "2027년 11월 5일");
        assert_eq!(super::korean_date("soon"), "soon");
    }
}
