use crate::code_editor_renderer::BLACK_COLOR; // TODO: maybe this should be part of this module instead?
use crate::colorscheme;
use crate::ui_toolkit::{Color, DrawFnRef, UiToolkit};
use cs::{lang, structs, EnvGenie};

pub fn render_struct_identifier<T: UiToolkit>(strukt: &structs::Struct,
                                              render_name_with_type_fn: &dyn Fn(&str,
                                                      Color,
                                                      &lang::Type)
                                                      -> T::DrawResult)
                                              -> T::DrawResult {
    let typ = lang::Type::from_spec(strukt);
    render_name_with_type_fn(&strukt.name, colorscheme!(cool_color), &typ)
}

pub fn render_name_with_type_definition<T: UiToolkit>(ui_toolkit: &T,
                                                      env_genie: &EnvGenie,
                                                      name: &str,
                                                      color: Color,
                                                      typ: &lang::Type)
                                                      -> T::DrawResult {
    let sym = env_genie.get_symbol_for_type(typ);
    let darker_color = darken(color);
    ui_toolkit.draw_all_on_same_line(&[&|| ui_toolkit.draw_buttony_text(&sym, darker_color),
                                       &|| ui_toolkit.draw_buttony_text(name, color)])
}

pub fn draw_nested_borders_around<T: UiToolkit>(ui_toolkit: &T,
                                                draw_fn: DrawFnRef<T>,
                                                nesting_level: u8)
                                                -> T::DrawResult {
    if nesting_level == 0 {
        return draw_fn();
    }
    let top_border_thickness = 1 + nesting_level + 1;
    let right_border_thickness = 1;
    let left_border_thickness = 1;
    let bottom_border_thickness = 1;

    ui_toolkit.draw_top_border_inside(BLACK_COLOR, top_border_thickness as u8, &|| {
                  ui_toolkit.draw_right_border_inside(BLACK_COLOR, right_border_thickness, &|| {
                      ui_toolkit.draw_left_border_inside(BLACK_COLOR, left_border_thickness, &|| {
                          ui_toolkit.draw_bottom_border_inside(BLACK_COLOR,
                                                               bottom_border_thickness,
                                                               draw_fn)
                      })
                  })
              })
}

pub fn darken(mut color: Color) -> Color {
    color[0] *= 0.75;
    color[1] *= 0.75;
    color[2] *= 0.75;
    color
}
