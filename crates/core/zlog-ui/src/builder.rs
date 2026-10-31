use zlog_core::{Extension, Registry};

use crate::chrome::{LogTable, MenuBar, StatusBar, ToolBar, WindowHost};
use crate::device::{Drivers, PortHub};
use crate::StartScreen;
use crate::{ContestEntry, ContestList, EntryStyle, Panel, Section, SettingsFile, SettingsPage};

pub mod order {
    pub const RULE_PLUGIN: i32 = 10;
    pub const RULES: i32 = 20;
    pub const FORMATS: i32 = 30;
    pub const LINKS: i32 = 40;
    pub const OBSERVERS: i32 = 50;
    pub const RIG: i32 = 100;
    pub const SO2R: i32 = 400;
    pub const KEYER: i32 = 410;
}

type Make<T> = Box<dyn Fn(&mut HostCtx<'_>) -> T + Send + Sync>;
type New<T> = Box<dyn Fn() -> T + Send + Sync>;
type Items<T> = Vec<Item<T>>;
type AddTo = Box<dyn Fn(&mut Drivers) + Send + Sync>;
pub type Look = (&'static str, fn(&egui::Context, &Section));

struct Item<T> {
    priority: i32,
    make: T,
}

fn sorted<T>(items: &[Item<T>]) -> Vec<&T> {
    let mut v: Vec<(i32, usize, &T)> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (it.priority, i, &it.make))
        .collect();
    v.sort_by_key(|(p, i, _)| (*p, *i));
    v.into_iter().map(|(_, _, t)| t).collect()
}

#[derive(Default)]
pub struct Builder {
    extensions: Items<New<Box<dyn Extension>>>,
    panels: Items<Make<Vec<Box<dyn Panel>>>>,
    pages: Items<Make<Vec<Box<dyn SettingsPage>>>>,
    drivers: Items<AddTo>,
    entry_styles: Items<New<Box<dyn EntryStyle>>>,
    settings_files: Items<New<Box<dyn SettingsFile>>>,
    contest_entries: Items<Make<Box<dyn ContestEntry>>>,
    contest_lists: Items<New<Box<dyn ContestList>>>,
    start_screen: Option<Make<Box<dyn StartScreen>>>,
    menu_bar: Option<New<Box<dyn MenuBar>>>,
    tool_bar: Option<New<Box<dyn ToolBar>>>,
    status_bar: Option<New<Box<dyn StatusBar>>>,
    log_table: Option<New<Box<dyn LogTable>>>,
    window_host: Option<New<Box<dyn WindowHost>>>,
    style: Option<Look>,
    subcommands: Vec<(&'static str, fn())>,
    start_time: Option<StartTimeDialog>,
}

pub type StartTimeDialog =
    fn(&egui::Context, &mut crate::time::StartTimeForm, &str) -> Option<crate::time::StartTimeDone>;

pub struct HostCtx<'b> {
    builder: &'b Builder,
    pub ports: PortHub,
    pub drivers: Drivers,
    pub registry: fn() -> Registry,
    pub toolbar: Option<crate::toolbar::Rows>,
}

impl HostCtx<'_> {
    pub fn new_drivers(&self) -> Drivers {
        self.builder.drivers()
    }

    pub fn contest_lists(&self) -> Vec<Box<dyn ContestList>> {
        self.builder
            .contest_lists_sorted()
            .into_iter()
            .map(|f| f())
            .collect()
    }

    pub fn contest_entries(&mut self) -> Vec<Box<dyn ContestEntry>> {
        let mut entries = crate::contest_groups(&(self.registry)());
        let builder = self.builder;
        for make in sorted(&builder.contest_entries) {
            entries.push(make(self));
        }
        entries
    }
}

impl Builder {
    pub fn new() -> Self {
        Builder::default()
    }

    pub fn start_time_dialog(&mut self, dialog: StartTimeDialog) {
        self.start_time = Some(dialog);
    }

    pub fn start_time(&self) -> Option<StartTimeDialog> {
        self.start_time
    }

    pub fn extension<E: Extension + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn() -> E + Send + Sync + 'static,
    ) {
        self.extensions.push(Item {
            priority,
            make: Box::new(move || Box::new(make())),
        });
    }

    pub fn panels(
        &mut self,
        priority: i32,
        make: impl Fn(&mut HostCtx<'_>) -> Vec<Box<dyn Panel>> + Send + Sync + 'static,
    ) {
        self.panels.push(Item {
            priority,
            make: Box::new(make),
        });
    }

    pub fn panel<P: Panel + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn(&mut HostCtx<'_>) -> P + Send + Sync + 'static,
    ) {
        self.panels(priority, move |ctx| vec![Box::new(make(ctx))]);
    }

    pub fn pages(
        &mut self,
        priority: i32,
        make: impl Fn(&mut HostCtx<'_>) -> Vec<Box<dyn SettingsPage>> + Send + Sync + 'static,
    ) {
        self.pages.push(Item {
            priority,
            make: Box::new(make),
        });
    }

    pub fn page<P: SettingsPage + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn(&mut HostCtx<'_>) -> P + Send + Sync + 'static,
    ) {
        self.pages(priority, move |ctx| vec![Box::new(make(ctx))]);
    }

    pub fn driver<T: ?Sized + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn() -> Box<T> + Send + Sync + 'static,
    ) {
        self.drivers.push(Item {
            priority,
            make: Box::new(move |d| d.add::<T>(make())),
        });
    }

    pub fn entry_style<S: EntryStyle + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn() -> S + Send + Sync + 'static,
    ) {
        self.entry_styles.push(Item {
            priority,
            make: Box::new(move || Box::new(make())),
        });
    }

    pub fn settings_file<F: SettingsFile + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn() -> F + Send + Sync + 'static,
    ) {
        self.settings_files.push(Item {
            priority,
            make: Box::new(move || Box::new(make())),
        });
    }

    pub fn contest_entry<E: ContestEntry + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn(&mut HostCtx<'_>) -> E + Send + Sync + 'static,
    ) {
        self.contest_entries.push(Item {
            priority,
            make: Box::new(move |ctx| Box::new(make(ctx))),
        });
    }

    pub fn contest_list<L: ContestList + 'static>(
        &mut self,
        priority: i32,
        make: impl Fn() -> L + Send + Sync + 'static,
    ) {
        self.contest_lists.push(Item {
            priority,
            make: Box::new(move || Box::new(make())),
        });
    }

    pub fn start_screen<S: StartScreen + 'static>(
        &mut self,
        make: impl Fn(&mut HostCtx<'_>) -> S + Send + Sync + 'static,
    ) {
        self.start_screen = Some(Box::new(move |ctx| Box::new(make(ctx))));
    }

    pub fn menu_bar<M: MenuBar + 'static>(&mut self, make: impl Fn() -> M + Send + Sync + 'static) {
        self.menu_bar = Some(Box::new(move || Box::new(make())));
    }

    pub fn tool_bar<T: ToolBar + 'static>(&mut self, make: impl Fn() -> T + Send + Sync + 'static) {
        self.tool_bar = Some(Box::new(move || Box::new(make())));
    }

    pub fn status_bar<S: StatusBar + 'static>(
        &mut self,
        make: impl Fn() -> S + Send + Sync + 'static,
    ) {
        self.status_bar = Some(Box::new(move || Box::new(make())));
    }

    pub fn log_table<G: LogTable + 'static>(
        &mut self,
        make: impl Fn() -> G + Send + Sync + 'static,
    ) {
        self.log_table = Some(Box::new(move || Box::new(make())));
    }

    pub fn window_host<W: WindowHost + 'static>(
        &mut self,
        make: impl Fn() -> W + Send + Sync + 'static,
    ) {
        self.window_host = Some(Box::new(move || Box::new(make())));
    }

    pub fn style(&mut self, section: &'static str, apply: fn(&egui::Context, &Section)) {
        self.style = Some((section, apply));
    }

    pub fn subcommand(&mut self, name: &'static str, run: fn()) {
        self.subcommands.push((name, run));
    }

    fn ctx(&self, registry: fn() -> Registry) -> HostCtx<'_> {
        HostCtx {
            builder: self,
            ports: PortHub::new(),
            drivers: self.drivers(),
            registry,
            toolbar: None,
        }
    }

    pub fn registry(&self) -> Registry {
        sorted(&self.extensions)
            .into_iter()
            .fold(Registry::new(), |r, make| r.with(make()))
    }

    pub fn drivers(&self) -> Drivers {
        let mut d = Drivers::new();
        for add in sorted(&self.drivers) {
            add(&mut d);
        }
        d
    }

    pub fn build_panels(&self, registry: fn() -> Registry) -> Vec<Box<dyn Panel>> {
        let mut ctx = self.ctx(registry);
        sorted(&self.panels)
            .into_iter()
            .flat_map(|make| make(&mut ctx))
            .collect()
    }

    pub fn build_pages(
        &self,
        registry: fn() -> Registry,
        toolbar: crate::toolbar::Rows,
    ) -> Vec<Box<dyn SettingsPage>> {
        let mut ctx = self.ctx(registry);
        ctx.toolbar = Some(toolbar);
        sorted(&self.pages)
            .into_iter()
            .flat_map(|make| make(&mut ctx))
            .collect()
    }

    pub fn build_entry_styles(&self) -> Vec<Box<dyn EntryStyle>> {
        sorted(&self.entry_styles)
            .into_iter()
            .map(|f| f())
            .collect()
    }

    pub fn build_settings_files(&self) -> Vec<Box<dyn SettingsFile>> {
        sorted(&self.settings_files)
            .into_iter()
            .map(|f| f())
            .collect()
    }

    fn contest_lists_sorted(&self) -> Vec<&(dyn Fn() -> Box<dyn ContestList> + Send + Sync)> {
        sorted(&self.contest_lists)
            .into_iter()
            .map(|b| b.as_ref())
            .collect()
    }

    pub fn build_start_screen(&self, registry: fn() -> Registry) -> Option<Box<dyn StartScreen>> {
        let make = self.start_screen.as_ref()?;
        Some(make(&mut self.ctx(registry)))
    }

    pub fn build_menu_bar(&self) -> Option<Box<dyn MenuBar>> {
        self.menu_bar.as_ref().map(|f| f())
    }

    pub fn build_tool_bar(&self) -> Option<Box<dyn ToolBar>> {
        self.tool_bar.as_ref().map(|f| f())
    }

    pub fn build_status_bar(&self) -> Option<Box<dyn StatusBar>> {
        self.status_bar.as_ref().map(|f| f())
    }

    pub fn build_log_table(&self) -> Option<Box<dyn LogTable>> {
        self.log_table.as_ref().map(|f| f())
    }

    pub fn build_window_host(&self) -> Option<Box<dyn WindowHost>> {
        self.window_host.as_ref().map(|f| f())
    }

    pub fn look(&self) -> Option<Look> {
        self.style
    }

    pub fn subcommand_of(&self, name: &str) -> Option<fn()> {
        self.subcommands
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, f)| *f)
    }
}
