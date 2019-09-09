use crate::color_schemes::colorscheme2;
use crate::colorscheme;
use crate::ui_toolkit::UiToolkit;

macro_rules! render_theme_color_picker_with_label {
    ($renderer:expr, $label:expr, $attr_name:ident) => {{
        let current_color = colorscheme!($attr_name);
        $renderer.ui_toolkit
                 .draw_color_picker_with_label($label, current_color, |color| {
                     colorscheme2().$attr_name = color;
                 })
    }};
}

pub struct ThemeEditorRenderer<'a, T: UiToolkit> {
    ui_toolkit: &'a T,
}

impl<'a, T: UiToolkit> ThemeEditorRenderer<'a, T> {
    pub fn new(ui_toolkit: &'a T) -> Self {
        Self { ui_toolkit }
    }

    pub fn render(&self) -> T::DrawResult {
        self.ui_toolkit.draw_all(&[&|| {
                                       render_theme_color_picker_with_label!(self,
                                                  "Active titlebar bg",
                                                  titlebar_active_bg_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Titlebar bg",
                                                                             titlebar_bg_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Input bg color",
                                                                             input_bg_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Child region bg color",
                                                                             child_region_bg_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Window bg color",
                                                                             window_bg_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Action color",
                                                                             action_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Adding color",
                                                                             adding_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Cool color",
                                                                             cool_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Button active color",
                                                                             button_active_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Button hover color",
                                                                             button_hover_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Literal bg color",
                                                                             literal_bg_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Danger color",
                                                                             danger_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Selection overlay color",
                                                                             selection_overlay_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Separator color",
                                                                             separator_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Text color",
                                                                             text_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Variable color",
                                                                             variable_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Warning color",
                                                                             warning_color)
                                   },
                                   &|| {
                                       render_theme_color_picker_with_label!(self,
                                                                             "Menubar bg",
                                                                             menubar_color)
                                   }])
    }
}
