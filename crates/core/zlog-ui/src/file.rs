use crate::{Command, SectionInfo, Sections};

pub struct SettingsEnv<'a> {
    pub sections: &'a mut Sections,
    pub infos: &'a [SectionInfo],
    pub language: &'a str,
    pub save: &'a dyn Fn(&Sections) -> std::io::Result<()>,
    pub migrations: &'a [crate::migration::Migration],
}

pub trait SettingsFile {
    fn id(&self) -> &str;

    fn commands(&self, language: &str) -> Vec<Command>;

    fn run_command(&mut self, id: &str, env: &mut SettingsEnv<'_>);

    fn dialogs(&mut self, ctx: &egui::Context, env: &mut SettingsEnv<'_>) -> bool;

    fn modal(&self) -> bool {
        false
    }
}
