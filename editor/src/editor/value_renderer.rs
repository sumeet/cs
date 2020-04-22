// TODO: many of the draw functions in here are copy and pasted from CodeEditorRenderer...
// ... i couldn't find a good way of sharing the code, but i think i might need to eventually.
// for now it's just copy+paste
use crate::align;
use crate::code_rendering::{
    render_name_with_type_definition, render_struct_field, render_struct_field_label,
    render_struct_identifier,
};
use crate::colorscheme;
use crate::ui_toolkit::{Color, UiToolkit};
use cs::env::ExecutionEnvironment;
use cs::lang::{StructValues, Value};
use cs::{lang, structs, EnvGenie};
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
            Value::Struct { struct_id, values } => return self.render_struct(struct_id, values),
            Value::Future(_) => {
                panic!("let's worry about futures later, they're not even in the example")
            }
            Value::Enum { .. } => {
                panic!("let's worry about enums later, they're not even in the example")
            }
        };
        self.draw_buttony_text_hardcoded_color(&label)
    }

    fn render_struct(&self, struct_id: &lang::ID, values: &StructValues) -> T::DrawResult {
        let strukt = self.env_genie.find_struct(*struct_id).unwrap();
        align!(T::self.ui_toolkit,
               &|| self.render_struct_identifier(strukt),
               strukt.fields.iter().map(|strukt_field| {
                                       move || {
                                           let value = values.get(&strukt_field.id).unwrap();
                                           self.render_struct_field_value(strukt_field, value)
                                       }
                                   }))
    }

    fn render_number(&self, value: &i128) -> T::DrawResult {
        self.ui_toolkit
            .draw_buttony_text(&value.to_string(), colorscheme!(literal_bg_color))
    }

    fn render_string(&self, value: &str) -> T::DrawResult {
        self.draw_buttony_text(&format!("\u{F10D} {} \u{F10E}", value),
                               colorscheme!(literal_bg_color))
    }

    fn render_struct_identifier(&self, strukt: &structs::Struct) -> T::DrawResult {
        render_struct_identifier::<T>(strukt, &|name, color, typ| {
            render_name_with_type_definition(self.ui_toolkit, &self.env_genie, name, color, typ)
        })
    }

    fn render_struct_field_value(&self,
                                 strukt_field: &structs::StructField,
                                 value: &lang::Value)
                                 -> T::DrawResult {
        render_struct_field(self.ui_toolkit,
                            &|| {
                                render_struct_field_label(self.ui_toolkit,
                                                          &self.env_genie,
                                                          strukt_field)
                            },
                            &|| Self::new(self.env, value, self.ui_toolkit).render())
    }

    fn draw_buttony_text(&self, label: &str, color: Color) -> T::DrawResult {
        self.ui_toolkit.draw_buttony_text(label, color)
    }

    fn draw_buttony_text_hardcoded_color(&self, label: &str) -> T::DrawResult {
        self.draw_buttony_text(label, colorscheme!(menubar_color))
    }
}
