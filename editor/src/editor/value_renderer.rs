// TODO: many of the draw functions in here are copy and pasted from CodeEditorRenderer...
// ... i couldn't find a good way of sharing the code, but i think i might need to eventually.
// for now it's just copy+paste
use crate::align;
use crate::code_rendering::{render_name_with_type_definition, render_struct_identifier};
use crate::colorscheme;
use crate::ui_toolkit::{Color, UiToolkit};
use cs::env::ExecutionEnvironment;
use cs::lang::Value;
use cs::{lang, EnvGenie};
use lazy_static::lazy_static;
use std::cell::RefCell;

lazy_static! {
    static ref TRUE_LABEL: String = format!("{} True", lang::BOOLEAN_TYPESPEC.symbol);
    static ref FALSE_LABEL: String = format!("{} False", lang::BOOLEAN_TYPESPEC.symbol);
    static ref NULL_LABEL: String = format!("{} Null", lang::NULL_TYPESPEC.symbol);
}

pub struct ValueRenderer<'a, T: UiToolkit> {
    nesting_level: RefCell<u8>,
    env_genie: EnvGenie<'a>,
    #[allow(unused)]
    env: &'a ExecutionEnvironment,
    value: &'a lang::Value,
    ui_toolkit: &'a T,
}

impl<'a, T: UiToolkit> ValueRenderer<'a, T> {
    pub fn new(env: &'a ExecutionEnvironment, value: &'a lang::Value, ui_toolkit: &'a T) -> Self {
        let env_genie = EnvGenie::new(env);
        Self { env,
               env_genie,
               nesting_level: RefCell::new(0),
               value,
               ui_toolkit }
    }

    pub fn render(&self) -> T::DrawResult {
        let label = match self.value {
            Value::Null => NULL_LABEL.clone(),
            Value::Boolean(bool) => {
                if *bool {
                    TRUE_LABEL.clone()
                } else {
                    FALSE_LABEL.clone()
                }
            }
            Value::String(string) => return self.render_string(string),
            Value::Number(num) => return self.render_number(num),
            Value::List(_) => {
                panic!("let's worry about lists later, they're not even in the example")
            }
            Value::Struct { struct_id, values } => {
                return align!(T::self.ui_toolkit,
                              &|| self.render_struct_symbol_and_name_button(struct_id),
                              values.iter().map(|(struct_field_id, value)| {
                                               move || {
                                                   self.render_struct_field_value(struct_id,
                                                                                  struct_field_id,
                                                                                  value)
                                               }
                                           }))
            }
            Value::Future(_) => {
                panic!("let's worry about lists later, they're not even in the example")
            }
            Value::Enum { .. } => {
                panic!("let's worry about lists later, they're not even in the example")
            }
        };
        self.draw_buttony_text_hardcoded_color(&label)
    }

    fn render_number(&self, value: &i128) -> T::DrawResult {
        self.ui_toolkit
            .draw_buttony_text(&value.to_string(), colorscheme!(literal_bg_color))
    }

    fn render_string(&self, value: &str) -> T::DrawResult {
        self.draw_buttony_text(&format!("\u{F10D} {} \u{F10E}", value),
                               colorscheme!(literal_bg_color))
    }

    fn render_struct_symbol_and_name_button(&self, struct_id: &lang::ID) -> T::DrawResult {
        let strukt = self.env_genie.find_struct(*struct_id).unwrap();
        render_struct_identifier::<T>(strukt, &|name, color, typ| {
            render_name_with_type_definition(self.ui_toolkit, &self.env_genie, name, color, typ)
        })
    }

    fn render_struct_field_value(&self,
                                 struct_id: &lang::ID,
                                 struct_field_id: &lang::ID,
                                 value: &lang::Value)
                                 -> T::DrawResult {
        let (_strukt, strukt_field) = self.env_genie
                                          .get_struct_and_field(*struct_id, *struct_field_id)
                                          .unwrap();
        let type_display_info = self.env_genie
                                    .get_type_display_info(&strukt_field.field_type)
                                    .unwrap();
        let arg_name = &strukt_field.name;
        self.ui_toolkit.draw_all_on_same_line(&[
            &|| self.draw_buttony_text_hardcoded_color(&format!("{} {}", type_display_info.symbol, arg_name)),
            &|| Self::new(self.env, value, self.ui_toolkit).render(),
        ])
    }

    fn draw_buttony_text(&self, label: &str, color: Color) -> T::DrawResult {
        self.ui_toolkit.draw_buttony_text(label, color)
    }

    fn draw_buttony_text_hardcoded_color(&self, label: &str) -> T::DrawResult {
        self.draw_buttony_text(label, colorscheme!(menubar_color))
    }
}
