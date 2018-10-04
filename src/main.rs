#![feature(pattern_parentheses)]
#![feature(unboxed_closures)]
#![feature(specialization)]
#![feature(nll)]
#![feature(arbitrary_self_types)]

#[cfg(feature = "default")]
extern crate glium;
#[cfg(feature = "default")]
#[macro_use]
extern crate imgui;
#[cfg(feature = "default")]
extern crate imgui_sys;
#[cfg(feature = "default")]
extern crate imgui_glium_renderer;
#[cfg(feature = "default")]
mod imgui_support;
#[cfg(feature = "default")]
mod imgui_toolkit;

#[cfg(feature = "javascript")]
#[macro_use]
extern crate stdweb;

#[cfg(feature = "javascript")]
#[macro_use]
extern crate yew;

#[cfg(feature = "javascript")]
mod yew_toolkit;

#[macro_use]
extern crate objekt;

extern crate debug_cell;
use debug_cell::RefCell;

//use std::cell::RefCell;
use std::rc::Rc;

mod lang;
mod env;

use self::env::{ExecutionEnvironment};
use self::lang::{
    Value,CodeNode,Function,FunctionCall,StringLiteral,ID,Error,Assignment,Block,
    VariableReference};

const BLUE_COLOR: [f32; 4] = [0.196, 0.584, 0.721, 1.0];
const GREY_COLOR: [f32; 4] = [0.521, 0.521, 0.521, 1.0];
const PURPLE_COLOR: [f32; 4] = [0.486, 0.353, 0.952, 1.0];
const CLEAR_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

#[cfg(feature = "default")]
pub fn draw_app(app: Rc<CSApp>) {
    imgui_toolkit::draw_app(Rc::clone(&app));
}

#[cfg(feature = "javascript")]
pub fn draw_app(app: Rc<CSApp>) {
    yew_toolkit::draw_app(Rc::clone(&app));
}


fn main() {
    let app = Rc::new(CSApp::new());
    draw_app(app);
}

#[derive(Clone)]
struct Print {}

impl Function for Print {
    fn call(&self, env: &mut ExecutionEnvironment, args: Vec<Value>) -> Value {
        match args.as_slice() {
            [Value::String(string)] =>  {
                env.println(string);
                Value::Null
            }
            _ => Value::Result(Result::Err(Error::ArgumentError))
        }
    }

    fn name(&self) -> &str {
        "Print"
    }
}

pub struct Controller {
    execution_environment: ExecutionEnvironment,
    selected_node_id: Option<ID>,
    loaded_code: Option<CodeNode>,
}

impl<'a> Controller {
    fn new() -> Controller {
        Controller {
            execution_environment: ExecutionEnvironment::new(),
            selected_node_id: None,
            loaded_code: None,
        }
    }

    fn load_code(&mut self, code_node: &CodeNode) {
        self.loaded_code = Some(code_node.clone())
    }

    // should run the loaded code node
    fn run(&mut self, code_node: &CodeNode) {
        code_node.evaluate( &mut self.execution_environment);
    }

    fn read_console(&self) -> &str {
        &self.execution_environment.console
    }

    fn set_selected_node_id(&mut self, code_node_id: Option<ID>) {
        self.selected_node_id = code_node_id;
    }

    fn get_selected_node_id(&self) -> &Option<ID> {
        &self.selected_node_id
    }
}

pub struct CSApp {
    pub controller: Rc<RefCell<Controller>>,
}

trait UiToolkit {
    fn draw_window(&self, window_name: &str, f: &Fn());
    fn draw_empty_line(&self);
    fn draw_button<F: Fn() + 'static>(&self, label: &str, color: [f32; 4], f: F);
    fn draw_text_box(&self, text: &str);
    fn draw_next_on_same_line(&self);
    fn draw_text_input<F: Fn(&str) -> () + 'static, D: Fn() + 'static>(&self, existing_value: &str, onchange: F, ondone: D);
    fn focus_last_drawn_element(&self);
}

struct AppRenderer<'a, T> {
    ui_toolkit: &'a mut T,
    controller: Rc<RefCell<Controller>>,
}

impl<'a, T: UiToolkit> AppRenderer<'a, T> {
    fn render_console_window(&self) {
        let controller = self.controller.clone();
        self.ui_toolkit.draw_window("Console", &|| {
            self.ui_toolkit.draw_text_box(controller.borrow().read_console());
        })
    }

    fn render_code_window(&self) {
        let loaded_code = self.controller.borrow().loaded_code.clone();
        match loaded_code {
            None => {},
            Some(ref code) => {
                self.ui_toolkit.draw_window(&code.description(), &|| {
                    self.render_code(code);
                    self.ui_toolkit.draw_empty_line();
                    self.render_run_button(code);
                })
            }
        }
    }

    fn render_code(&self, code_node: &CodeNode) {
        match code_node {
            CodeNode::FunctionCall(function_call) => {
                self.render_function_call(&function_call);
            }
            CodeNode::StringLiteral(string_literal) => {
                self.render_string_literal(&string_literal);
            }
            CodeNode::Assignment(assignment) => {
                self.render_assignment(&assignment);
            }
            CodeNode::Block(block) => {
                self.render_block(&block)
            }
            CodeNode::VariableReference(variable_reference) => {
                self.render_variable_reference(&variable_reference)
            }
        }
    }

    fn render_assignment(&self, assignment: &Assignment) {
        self.render_inline_editable_button(
            &assignment.name,
            PURPLE_COLOR,
            &CodeNode::Assignment(assignment.clone())
        );
        self.ui_toolkit.draw_next_on_same_line();
        self.ui_toolkit.draw_button("=", CLEAR_COLOR, &|| {});
        self.ui_toolkit.draw_next_on_same_line();
        self.render_code(assignment.expression.as_ref())
    }

    fn render_variable_reference(&self, variable_reference: &VariableReference) {
        let mut controller = self.controller.borrow_mut();
        let loaded_code = controller.loaded_code.as_mut().unwrap();
        let assignment = loaded_code.find_node(variable_reference.assignment_id);
        if let(Some(CodeNode::Assignment(assignment))) = assignment {
            self.ui_toolkit.draw_button(&assignment.name, PURPLE_COLOR, &|| {});
        }
    }

    fn render_block(&self, block: &Block) {
        for expression in &block.expressions {
            self.render_code(expression)
        }
    }

    fn render_function_call(&self, function_call: &FunctionCall) {
        self.ui_toolkit.draw_button(function_call.function.name(), BLUE_COLOR, &|| {});
        for code_node in &function_call.args {
            self.ui_toolkit.draw_next_on_same_line();
            self.render_code(code_node)
        }
    }

    fn render_string_literal(&self, string_literal: &StringLiteral) {
        self.render_inline_editable_button(
            &string_literal.value,
            CLEAR_COLOR,
            &CodeNode::StringLiteral(string_literal.clone())
        )
    }

    fn render_run_button(&self, code_node: &CodeNode) {
        let controller = self.controller.clone();
        let code_node = code_node.clone();
        self.ui_toolkit.draw_button("Run", GREY_COLOR, move ||{
            let mut controller = controller.borrow_mut();
            controller.run(&code_node);
        })
    }

    fn render_inline_editable_button(&self, label: &str, color: [f32; 4], code_node: &CodeNode) {
        if Some(code_node.id()) != *self.controller.borrow().get_selected_node_id() {
            self.render_inline_editable_button_when_not_editing(label, color, code_node);
        } else {
            self.render_inline_editable_button_when_editing(code_node);
        }
    }

    fn render_inline_editable_button_when_not_editing(&self, label: &str, color: [f32; 4], code_node: &CodeNode) {
        let controller = self.controller.clone();
        let id = code_node.id();
        self.ui_toolkit.draw_button(label, color, move || {
            let mut controller = controller.borrow_mut();
            controller.set_selected_node_id(Some(id))
        });
    }


    fn render_inline_editable_button_when_editing(&self, code_node: &CodeNode) {
        self.draw_inline_editor(code_node);
        self.ui_toolkit.focus_last_drawn_element();
    }

    fn draw_inline_editor(&self, code_node: &CodeNode) {
        match code_node {
            CodeNode::StringLiteral(string_literal) => {
                let sl = string_literal.clone();
                self.draw_inline_text_editor(
                    &string_literal.value,
                    move |new_value| {
                        let mut new_literal = sl.clone();
                        new_literal.value = new_value.to_string();
                        CodeNode::StringLiteral(new_literal)
                    })
            },
            CodeNode::Assignment(assignment) => {
                let a = assignment.clone();
                self.draw_inline_text_editor(
                    &assignment.name,
                    move |new_value| {
                        let mut new_assignment = a.clone();
                        new_assignment.name = new_value.to_string();
                        CodeNode::Assignment(new_assignment)
                    })
            },
            _ => panic!("unsupported inline editor for {:?}", code_node)
        }
    }

    fn draw_inline_text_editor<F: Fn(&str) -> CodeNode + 'static>(&self, initial_value: &str, new_node_fn: F) {
        let controller = Rc::clone(&self.controller);
        let controller2 = Rc::clone(&self.controller);
        self.ui_toolkit.draw_text_input(
            initial_value,
            move |new_value| {
                let new_node = new_node_fn(new_value);
                controller.borrow_mut().loaded_code.as_mut().unwrap().replace(&new_node)
            },
            move || {
                controller2.borrow_mut().set_selected_node_id(None)
            }

        )
    }
}

impl CSApp {
    fn new() -> CSApp {
//        // code print hello world
//        let mut args: Vec<CodeNode> = Vec::new();
//        let string_literal = StringLiteral { value: "HW".to_string(), id: 1};
//        args.push(CodeNode::StringLiteral(string_literal));
//        let function_call = FunctionCall {
//            function: Box::new(Print {}),
//            args: args,
//            id: 2,
//        };
//        let print_hello_world: CodeNode = CodeNode::FunctionCall(function_call);

        // code assignment
        let string_literal = CodeNode::StringLiteral(
            StringLiteral { value: "HW".to_string(), id: 1 });
        let assignment = CodeNode::Assignment(Assignment {
            name: "my variable".to_string(),
            expression: Box::new(string_literal),
            id: 2
        });
        let variable_reference = CodeNode::VariableReference(VariableReference {
            assignment_id: assignment.id(),
            id: 219,
        });
        let function_call = CodeNode::FunctionCall(FunctionCall {
            function: Box::new(Print {}),
            args: vec![variable_reference],
            id: 3,
        });
        let code_block = CodeNode::Block(Block {
            expressions: vec![assignment, function_call],
            id: 4,
        });


        let app = CSApp {
            controller: Rc::new(RefCell::new(Controller::new())),
        };
        app.controller.borrow_mut().load_code(&code_block);
        app
    }

    fn draw<T: UiToolkit>(self: &Rc<CSApp>, ui_toolkit: &mut T) {
        let app_renderer = AppRenderer {
            ui_toolkit: ui_toolkit,
            controller: self.controller.clone(),
        };

        app_renderer.render_code_window();
        app_renderer.render_console_window();
    }
}
