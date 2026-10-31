use egui::{ComboBox, Id, Response, TextEdit, Ui};

#[derive(Clone, Copy, Default)]
struct Shared {
    active: bool,
    right: f32,
    label: f32,
    next: f32,
}

fn shared_id() -> Id {
    Id::unique("zlog_ui_form_shared")
}

pub(crate) fn shared_label(ui: &Ui, own: f32) -> f32 {
    ui.ctx().data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Shared>(shared_id());
        if !s.active {
            return own;
        }
        s.next = s.next.max(own);
        s.label.max(own)
    })
}

pub(crate) fn shared_room(ui: &Ui) -> Option<f32> {
    let s = ui.ctx().data(|d| d.get_temp::<Shared>(shared_id()))?;
    s.active.then(|| s.right - ui.cursor().min.x)
}

pub fn shared<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let right = ui.max_rect().right();
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Shared>(shared_id());
        s.active = true;
        s.right = right;
        s.next = 0.0;
    });
    let r = add(ui);
    let changed = ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Shared>(shared_id());
        s.active = false;
        let changed = (s.next - s.label).abs() > 0.5;
        s.label = s.next;
        changed
    });
    if changed {
        ctx.request_discard("form label column");
    }
    r
}

pub struct Form {
    field: f32,
    left: f32,
}

impl Form {
    pub fn width(&self) -> f32 {
        self.field
    }

    pub fn text(&self, ui: &mut Ui, edit: TextEdit<'_>) -> Response {
        let r = crate::sized_field(ui, edit, self.field);
        self.seen(ui, &r);
        r
    }

    pub fn multiline(&self, ui: &mut Ui, edit: TextEdit<'_>) -> Response {
        let r = ui.add(edit.desired_width(self.field));
        self.seen(ui, &r);
        r
    }

    pub fn fit<R>(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        let size = egui::vec2(self.field, ui.spacing().interact_size.y);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(size, layout, add).inner
    }

    fn seen(&self, ui: &Ui, r: &Response) {
        shared_label(ui, r.rect.min.x - self.left - ui.spacing().item_spacing.x);
    }

    pub fn combo(&self, id_salt: impl egui::AsIdSalt) -> ComboBox<'static> {
        ComboBox::from_id_salt(id_salt).width(self.field).truncate()
    }
}

pub fn form<R>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    labels: &[&str],
    add: impl FnOnce(&mut Ui, &Form) -> R,
) -> R {
    let form = Form {
        field: crate::form_field_width(ui, labels),
        left: ui.cursor().min.x,
    };
    let label = shared_label(ui, 0.0);
    egui::Grid::new(ui.make_persistent_id(id_salt))
        .num_columns(2)
        .min_col_width(label.max(ui.spacing().interact_size.x))
        .show(ui, |ui| add(ui, &form))
        .inner
}

pub fn text(ui: &mut Ui, edit: TextEdit<'_>) -> Response {
    let width = ui.available_width();
    ui.add(edit.desired_width(width))
}

pub fn combo(ui: &Ui, id_salt: impl egui::AsIdSalt) -> ComboBox<'static> {
    ComboBox::from_id_salt(id_salt).width(ui.available_width())
}

pub fn buttons<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal_wrapped(add).inner
}
