use zlog_ui::builder::order;
use zlog_ui::Builder;

pub fn register(b: &mut Builder) {
    zlog_std::register(b);
    b.extension(order::RULES, || zlog_cfg::Cfg);
    zlog_cfg::script::host::register(b);
    zlog_lua::register(b);
    b.contest_entry(10, |c| {
        zlog_cfg::window::ContestWindow::new(c.contest_lists())
    });
    b.extension(order::RULES + 1, || zlog_rule_ja::Japan);
    b.extension(order::RULES + 2, || zlog_rule_dx::World);
    b.extension(order::FORMATS, || zlog_qxsl::Qxsl);
    for (i, model) in zlog_rig::MODELS.iter().enumerate() {
        b.driver::<&'static zlog_rig::Model>(i as i32, move || Box::new(model));
    }
    b.driver::<zlog_rig::SimFactory>(0, || Box::new(zlog_rig::simulator as zlog_rig::SimFactory));
    zlog_after::register(b);
    zlog_audio::register(b);
    zlog_backup::register(b);
    zlog_check::register(b);
    zlog_cluster::register(b);
    zlog_country::register(b);
    zlog_cty::register(b);
    zlog_edit::register(b);
    zlog_form::register(b);
    zlog_guide::register(b);
    zlog_history::register(b);
    zlog_ini::register(b);
    zlog_keyer::register(b);
    zlog_keyer_line::register(b);
    zlog_keyer_sim::register(b);
    zlog_link::register(b);
    zlog_map::register(b);
    zlog_number::register(b);
    zlog_op::register(b);
    zlog_qsy::register(b);
    zlog_rate::register(b);
    zlog_rig::register(b);
    zlog_rtty::register(b);
    zlog_scope::register(b);
    zlog_score::register(b);
    zlog_search::register(b);
    zlog_summary::register(b);
    zlog_toml::register(b);
    zlog_voice::register(b);
    zlog_zylo::register(b);
    zlog_webhook::register(b);
    zlog_cw4isr::register(b);
    zlog_ft8::register(b);
    zlog_so2r::register(b);
    zlog_license::register(b);
    zlog_jarl::register(b);
    zlog_es::register(b);
    zlog_comport::register(b);
    zlog_keyer_winkey::register(b);
    zlog_keyer_if4cw::register(b);
    zlog_so2r_neo::register(b);
    zlog_so2r_otrsp::register(b);
    zlog_remote::register(b);
    zlog_ja6ycu::register(b);
    zlog_market::register(b);
}
