use debug_cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;
use std::iter;

use failure::{err_msg};
use failure::Error as Error;
use super::code_loading::{serialize};
use super::env::{ExecutionEnvironment};
use super::editor_views::{FunctionCallView};
use super::lang;
use super::code_generation;
use super::lang::{
    Value,CodeNode,Function,FunctionCall,FunctionReference,StringLiteral,ID,Error as LangError,Assignment,Block,
    VariableReference};
use super::itertools::Itertools;
use super::pystuff;
use super::indexmap;


pub const BLUE_COLOR: Color = [0.196, 0.584, 0.721, 1.0];
pub const YELLOW_COLOR: Color = [253.0 / 255.0, 159.0 / 255.0, 19.0 / 255.0, 1.0];
pub const BLACK_COLOR: Color = [0.0, 0.0, 0.0, 1.0];
pub const RED_COLOR: Color = [0.858, 0.180, 0.180, 1.0];
pub const GREY_COLOR: Color = [0.521, 0.521, 0.521, 1.0];
pub const PURPLE_COLOR: Color = [0.486, 0.353, 0.952, 1.0];
pub const CLEAR_COLOR: Color = [0.0, 0.0, 0.0, 0.0];

pub const PLACEHOLDER_ICON: &str = "\u{F071}";

pub type Color = [f32; 4];

// TODO: types of insert code generators
// 1: variable
// 2: function call to capitalize
// 3: new string literal
// 4: placeholder

#[derive(Clone)]
struct InsertCodeMenu {
    option_generators: Vec<Box<InsertCodeMenuOptionGenerator>>,
    selected_option_index: usize,
    search_params: CodeSearchParams,
    insertion_point: InsertionPoint,
}

impl InsertCodeMenu {
    fn new_expression_inside_code_block(insertion_point: InsertionPoint, env: &ExecutionEnvironment) -> Self {
        Self {
            // TODO: should probably be able to insert new assignment expressions as well
            option_generators: vec![Box::new(InsertFunctionOptionGenerator { all_funcs: env.list_functions() })],
            selected_option_index: 0,
            search_params: CodeSearchParams::empty(),
            insertion_point,
        }
    }

    fn fill_in_argument(argument: &lang::Argument, env: &ExecutionEnvironment, root_node: &CodeNode) -> Self {
        let genie = CodeGenie::new(root_node, env);
        let arg_type = genie.get_type_for_arg(argument.argument_definition_id);
        if arg_type.is_none() {
            panic!("h000000what. couldn't find the argument definition for this thing!")
        }
        let arg_type = arg_type.unwrap();

        // XXX: type ID will no longer be sufficient. i think we may need to hash together the child
        // type hashes together as well, or smth (for parameterized types)
        let assignments_by_type_id = genie.find_assignments_that_come_before_code(argument.id)
            .into_iter()
            .group_by(|assignment| {
                let assignment : Assignment = (**assignment).clone();
                genie.guess_type(&CodeNode::Assignment(assignment)).typespec.id
            })
            .into_iter()
            .map(|(id, assignments)| (id, assignments.cloned().collect::<Vec<Assignment>>()))
            .collect();

        Self {
            option_generators: vec![
                Box::new(InsertVariableReferenceOptionGenerator { assignments_by_type_id }),
                Box::new(InsertFunctionOptionGenerator { all_funcs: env.list_functions() }),
                Box::new(InsertLiteralOptionGenerator {}),
            ],
            selected_option_index: 0,
            search_params: CodeSearchParams::with_type(&arg_type),
            insertion_point: InsertionPoint::Argument(argument.id),
        }
    }

    fn selected_option_code(&self) -> Option<CodeNode> {
        Some(self.list_options().get(self.selected_option_index)?.new_node.clone())
    }

    fn select_next(&mut self) {
        if self.selected_option_index < self.list_options().len() - 1 {
            self.selected_option_index += 1;
        } else {
            self.selected_option_index = 0;
        }
    }

    fn set_search_str(&mut self, input_str: &str) {
        if input_str != self.search_params.input_str {
            self.search_params.input_str = input_str.to_string();
            self.selected_option_index = 0;
        }
    }

    // TODO: i think the selected option index can get out of sync with this generated list, leading
    // to a panic, say if someone types something and changes the number of options without changing
    // the selected index.
    fn list_options(&self) -> Vec<InsertCodeMenuOption> {
        let mut all_options : Vec<InsertCodeMenuOption> = self.option_generators
            .iter()
            .flat_map(|generator| generator.options(&self.search_params))
            .collect();
        all_options.get_mut(self.selected_option_index).as_mut()
            .map(|option| option.is_selected = true);
        all_options
    }
}

trait InsertCodeMenuOptionGenerator : objekt::Clone {
    fn options(&self, search_params: &CodeSearchParams) -> Vec<InsertCodeMenuOption>;
}

clone_trait_object!(InsertCodeMenuOptionGenerator);

#[derive(Clone)]
struct CodeSearchParams {
    return_type: Option<lang::Type>,
    input_str: String,
}

impl CodeSearchParams {
    fn empty() -> Self {
        Self { return_type: None, input_str: "".to_string() }
    }

    fn with_type(t: &lang::Type) -> Self {
        Self { return_type: Some(t.clone()), input_str: "".to_string() }
    }
}

#[derive(Clone)]
struct InsertCodeMenuOption {
    label: String,
    new_node: CodeNode,
    is_selected: bool,
}

#[derive(Clone)]
struct InsertFunctionOptionGenerator {
    all_funcs: Vec<Box<Function>>,
}

impl InsertCodeMenuOptionGenerator for InsertFunctionOptionGenerator {
    fn options(&self, search_params: &CodeSearchParams) -> Vec<InsertCodeMenuOption> {
        let mut functions = self.all_funcs.clone();
        let input_str = search_params.input_str.trim().to_lowercase();
        if !input_str.is_empty() {
            functions = functions.into_iter()
                .filter(|f| {
                    f.name().to_lowercase().contains(&input_str)
                }).collect()
        }
        if let(Some(ref return_type)) = search_params.return_type {
            functions = functions.into_iter()
                .filter(|f| f.returns().matches(&return_type)).collect()
        }
        functions.into_iter().map(|func| {
            InsertCodeMenuOption {
                label: func.name().to_string(),
                new_node: code_generation::new_function_call_with_placeholder_args(func.as_ref()),
                is_selected: false,
            }
        }).collect()
    }
}

#[derive(Clone)]
struct InsertVariableReferenceOptionGenerator {
    assignments_by_type_id: HashMap<ID, Vec<Assignment>>,
}

impl InsertCodeMenuOptionGenerator for InsertVariableReferenceOptionGenerator {
    fn options(&self, search_params: &CodeSearchParams) -> Vec<InsertCodeMenuOption> {
        let mut assignments = if let(Some(search_type)) = &search_params.return_type {
            // XXX: searching by typespec ID is fuxkkkkeeddddddd
            self.assignments_by_type_id.get(&search_type.typespec.id).map_or_else(
                || vec![],
                |assignments| assignments.iter()
                    .map(|assignment| assignment.clone()).collect()
            )
        } else {
            self.assignments_by_type_id.iter()
                .flat_map(|(_id, assignments)| assignments)
                .map(|assignment| assignment.clone())
                .collect()
        };

        let input_str = search_params.input_str.trim().to_lowercase();
        if !input_str.is_empty() {
            assignments = assignments.into_iter()
                .filter(|assignment| {
                    assignment.name.to_lowercase().contains(&input_str)
                }).collect()
        }

        assignments.into_iter().map(|assignment| {
            InsertCodeMenuOption {
                label: assignment.name.to_string(),
                new_node: code_generation::new_variable_reference(&assignment),
                is_selected: false,
            }
        }).collect()
    }
}

#[derive(Clone)]
struct InsertLiteralOptionGenerator {}

impl InsertCodeMenuOptionGenerator for InsertLiteralOptionGenerator {
    fn options(&self, search_params: &CodeSearchParams) -> Vec<InsertCodeMenuOption> {
        let mut options = vec![];
        let input_str = &search_params.input_str;
        if let(Some(ref return_type)) = search_params.return_type {
            if return_type.matches_spec(&lang::STRING_TYPESPEC) {
                options.push(
                    InsertCodeMenuOption {
                        label: format!("\u{f10d}{}\u{f10e}", input_str),
                        is_selected: false,
                        new_node: code_generation::new_string_literal(input_str)
                    }
                );
                // design decision made here: all placeholders have types. therefore, it is now
                // required for a placeholder node to have a type, meaning we need to know what the
                // type of a placeholder is to create it. under current conditions that's ok, but i
                // think we can make this less restrictive in the future if we need to
                options.push(
                    InsertCodeMenuOption {
                        label: format!("{} {}", PLACEHOLDER_ICON, input_str),
                        is_selected: false,
                        // XXX: omg, placholders will now need to hold a type instead of type id
                        new_node: code_generation::new_placeholder(input_str, return_type.typespec.id),
                    }
                );
            }
        }
        options
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InsertionPoint {
    Before(ID),
    After(ID),
    Argument(ID),
}

impl InsertionPoint {
    fn node_id(&self) -> ID {
        match *self {
            InsertionPoint::Before(id) => id,
            InsertionPoint::After(id) => id,
            InsertionPoint::Argument(id) => id,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Keypress {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
}

impl Keypress {
    pub fn new(key: Key, ctrl: bool, shift: bool) -> Keypress {
        Keypress { key, ctrl, shift }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Key {
    A,
    B,
    C,
    D,
    H,
    L,
    W,
    X,
    R,
    O,
    U,
    V,
    Tab,
    Escape,
    LeftArrow,
    RightArrow,
}

pub struct CodeGenie<'a> {
    code: &'a CodeNode,
    env: &'a ExecutionEnvironment,
}

impl<'a> CodeGenie<'a> {
    fn new(code_node: &'a CodeNode, env: &'a ExecutionEnvironment) -> Self {
        Self { code: code_node, env }
    }

    fn find_assignments_that_come_before_code(&self, node_id: ID) -> Vec<&Assignment> {
        let block_expression_id = self.find_expression_inside_block_that_contains(node_id);
        if block_expression_id.is_none() {
            return vec![]
        }
        let block_expression_id = block_expression_id.unwrap();
        match self.find_parent(block_expression_id) {
            Some(CodeNode::Block(block)) => {
                // if this dies, it means we found a block that's a parent of a block expression,
                // but then when we looked inside the block it didn't contain that expression. this
                // really shouldn't happen
                let position_in_block = block.expressions.iter()
                    .position(|code| code.id() == block_expression_id)
                    .unwrap();

                block.expressions.iter()
                    // position in the block is 0 indexed, so this will take every node up TO it
                    .take(position_in_block)
                    .map(|code| code.into_assignment())
                    .filter(|opt| opt.is_some())
                    .map(|opt| opt.unwrap())
                    .collect()
            },
            _ => vec![]
        }
    }

    fn find_expression_inside_block_that_contains(&self, node_id: ID) -> Option<ID> {
        let parent = self.code.find_parent(node_id);
        match parent {
            Some(CodeNode::Block(_)) => Some(node_id),
            Some(parent_node) => self.find_expression_inside_block_that_contains(
                parent_node.id()),
            None => None
        }
    }

    fn get_type_for_arg(&self, argument_definition_id: ID) -> Option<lang::Type> {
        for function in self.all_functions() {
            for arg_def in function.takes_args() {
                if arg_def.id == argument_definition_id {
                    return Some(arg_def.arg_type)
                }
            }
        }
        None
    }

    fn get_functions_returning_type(&self, t: &lang::Type) -> Vec<Box<Function>> {
        self.all_functions().into_iter()
            .filter(|func| func.returns().matches(t))
            .collect()
    }

    fn root(&self) -> &CodeNode {
        self.code
    }

    fn find_node(&self, id: ID) -> Option<&CodeNode> {
        self.code.find_node(id)
    }

    fn find_parent(&self, id: ID) -> Option<&CodeNode> {
        self.code.find_parent(id)
    }

    fn find_function(&self, id: ID) -> Option<&Box<Function>> {
        self.env.find_function(id)
    }

    fn all_functions(&self) -> Vec<Box<Function>> {
        self.env.list_functions()
    }

    pub fn guess_type(&self, code_node: &CodeNode) -> lang::Type {
        match code_node {
            CodeNode::FunctionCall(function_call) => {
                let func_id = function_call.function_reference().function_id;
                match self.find_function(func_id) {
                    Some(ref func) => func.returns().clone(),
                    // TODO: do we really want to just return Null if we couldn't find the function?
                    None => lang::Type::from_spec(&lang::NULL_TYPESPEC),
                }
            }
            CodeNode::StringLiteral(_) => {
                lang::Type::from_spec(&lang::STRING_TYPESPEC)
            }
            CodeNode::Assignment(assignment) => {
                self.guess_type(&assignment.expression)
            }
            CodeNode::Block(block) => {
                if block.expressions.len() > 0 {
                    let last_expression_in_block= &block.expressions[block.expressions.len() - 1];
                    self.guess_type(last_expression_in_block)
                } else {
                    lang::Type::from_spec(&lang::NULL_TYPESPEC)
                }
            }
            CodeNode::VariableReference(_) => {
                lang::Type::from_spec(&lang::NULL_TYPESPEC)
            }
            CodeNode::FunctionReference(_) => {
                lang::Type::from_spec(&lang::NULL_TYPESPEC)
            }
            CodeNode::FunctionDefinition(_) => {
                lang::Type::from_spec(&lang::NULL_TYPESPEC)
            }
            CodeNode::Argument(_) => {
                lang::Type::from_spec(&lang::NULL_TYPESPEC)
            }
            CodeNode::Placeholder(_) => {
                lang::Type::from_spec(&lang::NULL_TYPESPEC)
            }
        }
    }
}

pub struct Navigation<'a> {
    code_genie: &'a CodeGenie<'a>,
}

impl<'a> Navigation<'a> {
    pub fn new(code_genie: &'a CodeGenie) -> Self {
        Self { code_genie }
    }

    pub fn navigate_back_from(&self, code_node_id: Option<ID>) -> Option<ID> {
        if code_node_id.is_none() {
            return None
        }
        let mut go_back_from_id = code_node_id.unwrap();
        while let(Some(prev_node)) = self.prev_node_from(go_back_from_id) {
           if self.is_navigatable(prev_node) {
               return Some(prev_node.id())
           } else {
               go_back_from_id = prev_node.id()
           }
        }
        None
    }

    pub fn prev_node_from(&self, code_node_id: ID) -> Option<&CodeNode> {
        let parent = self.code_genie.find_parent(code_node_id);
        if parent.is_none() {
            return None
        }
        let parent = parent.unwrap();
        // first try the previous sibling
        if let(Some(previous_sibling)) = parent.previous_child(code_node_id) {
            // but since we're going back, if the previous sibling has children, then let's
            // select the last one. that feels more ergonomic while moving backwards
            let children = previous_sibling.all_children_dfs();
            if children.len() > 0 {
                return Some(children[children.len() - 1])
            } else {
                return Some(previous_sibling)
            }
        }

        // if there is no previous sibling, try the parent
        Some(parent)
    }

    pub fn navigate_forward_from(&self, code_node_id: Option<ID>) -> Option<ID> {
        let mut go_back_from_id = code_node_id;
        while let(Some(prev_node)) = self.next_node_from(go_back_from_id) {
            if self.is_navigatable(prev_node) {
                return Some(prev_node.id())
            } else {
                go_back_from_id = Some(prev_node.id())
            }
        }
        None
    }

    pub fn next_node_from(&self, code_node_id: Option<ID>) -> Option<&CodeNode> {
        if code_node_id.is_none() {
            return Some(self.code_genie.root())
        }

        let selected_node_id = code_node_id.unwrap();
        let selected_code = self.code_genie.find_node(selected_node_id).unwrap();
        let children = selected_code.children();
        let first_child = children.get(0);

        // if the selected node has children, then return the first child. depth first
        if let(Some(first_child)) = first_child {
            return Some(first_child)
        }

        let mut node_id_to_find_next_sibling_of = selected_node_id;
        while let(Some(parent))= self.code_genie.find_parent(node_id_to_find_next_sibling_of) {
            if let(Some(next_sibling)) = parent.next_child(node_id_to_find_next_sibling_of) {
                return Some(next_sibling)
            }
            // if there is no sibling, then try going to the next sibling of the parent, recursively
            node_id_to_find_next_sibling_of = parent.id()
        }
        None
    }
    // don't navigate to either blocks, or direct children of blocks
    fn is_navigatable(&self, code_node: &CodeNode) -> bool {
        match code_node {
            CodeNode::Block(_) => false,
            CodeNode::Assignment(_) => true,
            _ => {
                let parent = self.code_genie.find_parent(code_node.id());
                if parent.is_none() {
                    return false
                }
                let parent = parent.unwrap();
                match parent {
                    CodeNode::Block(_) => false,
                    _ => true,
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct TestResult {
    value: Value,
}

impl TestResult {
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

pub struct Controller {
    execution_environment: ExecutionEnvironment,
    selected_node_id: Option<ID>,
    pub editing: bool,
    insert_code_menu: Option<InsertCodeMenu>,
    loaded_code: Option<CodeNode>,
    error_console: String,
    type_by_id: indexmap::IndexMap<ID, lang::Type>,
    typespec_by_id: indexmap::IndexMap<ID, lang::TypeSpec>,
    mutation_master: MutationMaster,
    test_result_by_func_id: HashMap<ID, TestResult>,
}

impl<'a> Controller {
    pub fn new() -> Controller {
        Controller {
            execution_environment: ExecutionEnvironment::new(),
            selected_node_id: None,
            loaded_code: None,
            error_console: String::new(),
            insert_code_menu: None,
            editing: false,
            type_by_id: Self::build_types(),
            mutation_master: MutationMaster::new(),
            test_result_by_func_id: HashMap::new(),
            typespec_by_id: Self::init_typespecs(),
        }
    }

    fn init_typespecs() -> indexmap::IndexMap<ID, lang::TypeSpec> {
        let mut typespec_by_id : indexmap::IndexMap<ID, lang::TypeSpec> = indexmap::IndexMap::new();
        typespec_by_id.insert(lang::NULL_TYPESPEC.id, lang::NULL_TYPESPEC.clone());
        typespec_by_id.insert(lang::STRING_TYPESPEC.id, lang::STRING_TYPESPEC.clone());
        typespec_by_id.insert(lang::NUMBER_TYPESPEC.id, lang::NUMBER_TYPESPEC.clone());
        typespec_by_id.insert(lang::LIST_TYPESPEC.id, lang::LIST_TYPESPEC.clone());
        typespec_by_id
    }

    fn build_types() -> indexmap::IndexMap<ID, lang::Type> {
        let mut type_by_id : indexmap::IndexMap<ID, lang::Type> = indexmap::IndexMap::new();

        // XXX: HACKS: TEMP
        let null_type = lang::Type::from_spec(&lang::NULL_TYPESPEC);
        let string_type = lang::Type::from_spec(&lang::STRING_TYPESPEC);
        let number_type = lang::Type::from_spec(&lang::NUMBER_TYPESPEC);
        let list_of_numbers_type = lang::Type::with_params(
            &lang::LIST_TYPESPEC, vec![number_type.clone()]
        );
        let list_of_strings_type = lang::Type::with_params(
            &lang::LIST_TYPESPEC, vec![string_type.clone()]
        );

        for t in [null_type, string_type, number_type, list_of_numbers_type, list_of_strings_type].into_iter() {
            type_by_id.insert(t.id(), t.clone());
        }
        type_by_id
    }

    fn types(&self) -> Vec<&lang::Type> {
        self.type_by_id.values().collect()
    }


    fn typespecs(&self) -> Vec<&lang::TypeSpec> {
        self.typespec_by_id.values().collect()
    }

    fn typespec_names(&self) -> Vec<String> {
        self.typespecs().iter().map(|typespec| {
            format!("{} {}", typespec.symbol, typespec.readable_name)
        }).collect()
    }

    // TODO: return a result instead of returning nothing? it seems like there might be places this
    // thing can error
    fn insert_code(&mut self, code_node: CodeNode, insertion_point: InsertionPoint) {
        let genie = self.code_genie();
        let (next_selection_id, editing) = post_insertion_cursor(&code_node);
        let new_code = self.mutation_master.insert_code(
            code_node, insertion_point, genie.as_ref().unwrap(),
            self.selected_node_id);
        self.loaded_code.as_mut().unwrap().replace(&new_code);
        if editing {
            self.mark_as_editing(next_selection_id)
        } else {
            self.set_selected_node_id(Some(next_selection_id));
        }
    }

    fn get_test_result(&self, pyfunc: &pystuff::PyFunc) -> String {
        let test_result = self.test_result_by_func_id.get(&pyfunc.id());
        if let(Some(test_result)) = test_result {
            format!("{:?}", test_result.value)
        } else {
            "Test not run yet".to_string()
        }
    }

    fn run_test(&mut self, pyfunc: &pystuff::PyFunc) {
        // XXX lol ghetto
        let fc = code_generation::new_function_call_with_placeholder_args(pyfunc);
        let result = TestResult::new(self.run(&fc));
        self.test_result_by_func_id.insert(pyfunc.id(), result);
    }

    fn undo(&mut self) {
        if let(Some(previous_root)) = self.mutation_master.undo() {
            self.loaded_code.as_mut().unwrap().replace(&previous_root.code_node);
            self.selected_node_id = previous_root.cursor_position;
        }
    }

    fn redo(&mut self) {
        if let(Some(next_root)) = self.mutation_master.redo() {
            self.loaded_code.as_mut().unwrap().replace(&next_root.code_node);
            self.selected_node_id = next_root.cursor_position;
        }
    }

    fn delete_selected_code(&mut self) {
        let node_to_delete = self.get_selected_node().cloned().unwrap();
        let genie = self.code_genie();
        let new_code = self.mutation_master.delete_code(
            &node_to_delete, genie.as_ref().unwrap(), self.selected_node_id);
        self.loaded_code.as_mut().unwrap().replace(&new_code);
        // TODO: intelligently select a nearby node to select after deleting
        self.set_selected_node_id(None);
    }

    fn select_current_line(&mut self) {
        let genie = self.code_genie();
        if genie.is_none() || self.selected_node_id.is_none() {
            return
        }
        let genie = genie.unwrap();
        let selected_id = self.selected_node_id.unwrap();
        if let(Some(code_id)) = genie.find_expression_inside_block_that_contains(selected_id) {
            self.set_selected_node_id(Some(code_id))
        }
    }

    pub fn hide_insert_code_menu(&mut self) {
        self.insert_code_menu = None;
        self.editing = false
    }

    pub fn insertion_point(&self) -> Option<InsertionPoint> {
        match self.insert_code_menu.as_ref() {
            None => None,
            Some(menu) => Some(menu.insertion_point),
        }
    }

    pub fn handle_keypress_in_code_window(&mut self, keypress: Keypress) {
        if keypress.key == Key::Escape {
            self.handle_cancel();
            return
        }
        // don't perform any commands when in edit mode
        match (self.editing, keypress.key) {
            (false, Key::B) | (false, Key::LeftArrow) | (false, Key::H) => {
                self.try_select_back_one_node()
            },
            (false, Key::W) | (false, Key::RightArrow) | (false, Key::L) => {
                self.try_select_forward_one_node()
            },
            (false, Key::C) => {
                if let(Some(id)) = self.selected_node_id {
                    self.mark_as_editing(id)
                }
            },
            (false, Key::D) => {
                self.delete_selected_code()
            },
            (false, Key::R) => {
                if keypress.ctrl && keypress.shift {
                    self.run(&self.loaded_code.as_ref().unwrap().clone());
                } else if keypress.ctrl {
                    self.redo()
                }
            },
            (false, Key::O) => {
                if keypress.shift {
                    self.set_insertion_point_on_previous_line_in_block()
                } else {
                    self.set_insertion_point_on_next_line_in_block()
                }
            },
            (false, Key::U) => {
                self.undo()
            },
            (false, Key::V) if keypress.shift => {
                self.select_current_line()
            },
            (_, Key::Tab) => {
                self.insert_code_menu.as_mut()
                    .map(|menu| menu.select_next());
            }
            _ => {},
        }
    }

    fn handle_cancel(&mut self) {
        self.editing = false;
        if self.insert_code_menu.is_none() { return }

        match self.insert_code_menu.as_ref().unwrap().insertion_point {
            InsertionPoint::After(id) => self.selected_node_id = Some(id),
            InsertionPoint::Before(id) => self.selected_node_id = Some(id),
            InsertionPoint::Argument(id) => self.selected_node_id = Some(id),
        }
        self.hide_insert_code_menu()
    }

    fn set_insertion_point_on_previous_line_in_block(&mut self) {
        if let(Some(expression_id)) = self.currently_focused_block_expression() {
            self.insert_code_menu = Some(InsertCodeMenu::new_expression_inside_code_block(
                InsertionPoint::Before(expression_id),
                &self.execution_environment,
            ));
            self.editing = true;
            self.selected_node_id = None;
        } else {
            self.hide_insert_code_menu()
        }
    }

    fn set_insertion_point_on_next_line_in_block(&mut self) {
        if let(Some(expression_id)) = self.currently_focused_block_expression() {
            self.insert_code_menu = Some(InsertCodeMenu::new_expression_inside_code_block(
                InsertionPoint::After(expression_id),
                &self.execution_environment,
            ));
            self.editing = true;
            self.selected_node_id = None;
        } else {
            self.hide_insert_code_menu()
        }
    }

    fn mark_as_editing(&mut self, node_id: ID) {
        let genie = self.code_genie();
        if genie.is_none() {
            return
        }
        let genie = genie.unwrap();
        match genie.find_node(node_id) {
            Some(CodeNode::Argument(argument)) => {
                self.insert_code_menu = Some(
                    InsertCodeMenu::fill_in_argument(
                        argument,
                        &self.execution_environment,
                        self.loaded_code.as_ref().unwrap()));
            }
            _ => ()
        }
        self.selected_node_id = Some(node_id);
        self.editing = true;
    }

    fn currently_focused_block_expression(&self) -> Option<ID> {
        self.code_genie()?
            .find_expression_inside_block_that_contains(self.selected_node_id?)
    }

    fn code_genie(&'a self) -> Option<CodeGenie> {
        Some(CodeGenie::new(
            self.loaded_code.as_ref()?,
            &self.execution_environment,
        ))
    }

    pub fn try_select_back_one_node(&mut self) {
        let genie = self.code_genie();
        let navigation = Navigation::new(genie.as_ref().unwrap());
        if let(Some(node_id)) = navigation.navigate_back_from(self.selected_node_id) {
            self.set_selected_node_id(Some(node_id))
        }
    }

    pub fn try_select_forward_one_node(&mut self) {
        let genie = self.code_genie();
        let navigation = Navigation::new(genie.as_ref().unwrap());
        if let(Some(node_id)) = navigation.navigate_forward_from(self.selected_node_id) {
            self.set_selected_node_id(Some(node_id))
        }


    }

    pub fn select_loaded_code_if_nothing_selected(&mut self) -> Result<bool,Error> {
        if self.loaded_code.is_none() { return Err(err_msg("No code loaded")) }
        let loaded_code = self.loaded_code.as_ref().unwrap().clone();
        if self.get_selected_node_id().is_none() {
            self.set_selected_node_id(Some(loaded_code.id()));
            return Ok(true)
        }
        Ok(false)
    }

    pub fn set_typespec(&mut self, pyfunc: &pystuff::PyFunc,
                        typespec: &lang::TypeSpec, nesting_level: &Vec<usize>) {
        let mut newpyfunc = pyfunc.clone();

        let mut return_type = &mut newpyfunc.return_type;

        for param_index in nesting_level.into_iter() {
            return_type = &mut return_type.params[*param_index]
        }

        return_type.typespec = typespec.clone();
        return_type.params.truncate(typespec.num_params);
        let num_missing_params = typespec.num_params - return_type.params.len();
        for _ in 0..num_missing_params {
            return_type.params.push(lang::Type::from_spec(&lang::NULL_TYPESPEC))
        }
        self.load_function(Box::new(newpyfunc))
    }

    pub fn load_function(&mut self, function: Box<Function>) {
        self.execution_environment.add_function(function.clone())
    }

    pub fn find_function(&self, id: ID) -> Option<&Box<Function>> {
        self.execution_environment.find_function(id)
    }

    pub fn load_code(&mut self, code_node: &CodeNode) {
        self.loaded_code = Some(code_node.clone());
        self.mutation_master.seed_initial_history(code_node)
    }

    // should run the loaded code node
    pub fn run(&mut self, code_node: &CodeNode) -> Value {
        let result = self.execution_environment.evaluate(code_node);
        match result {
            Value::Error(ref e) => {
                self.error_console.push_str(&format!("{:?}", e));
                self.error_console.push_str("\n");
            }
            _ => { }
        }
        result
    }

    pub fn read_console(&self) -> &str {
        &self.execution_environment.console
    }

    pub fn read_error_console(&self) -> &str {
        &self.error_console
    }

    pub fn set_selected_node_id(&mut self, code_node_id: Option<ID>) {
        self.selected_node_id = code_node_id;
    }

    pub fn get_selected_node_id(&self) -> &Option<ID> {
        &self.selected_node_id
    }

    pub fn get_selected_node(&self) -> Option<&CodeNode> {
        self.loaded_code.as_ref()?.find_node(self.selected_node_id?)
    }

}

pub trait UiToolkit {
    type DrawResult;

    fn draw_all(&self, draw_results: Vec<Self::DrawResult>) -> Self::DrawResult;
    fn draw_window<F: Fn(Keypress)>(&self, window_name: &str, draw_fn: &Fn() -> Self::DrawResult, handle_keypress: F) -> Self::DrawResult;
    fn draw_layout_with_bottom_bar(&self, draw_content_fn: &Fn() -> Self::DrawResult, draw_bottom_bar_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_empty_line(&self) -> Self::DrawResult;
    fn draw_separator(&self) -> Self::DrawResult;
    fn draw_text(&self, text: &str) -> Self::DrawResult;
    fn draw_button<F: Fn() + 'static>(&self, label: &str, color: Color, f: F) -> Self::DrawResult;
    fn draw_small_button<F: Fn() + 'static>(&self, label: &str, color: Color, f: F) -> Self::DrawResult;
    fn draw_text_box(&self, text: &str) -> Self::DrawResult;
    fn draw_text_input<F: Fn(&str) -> () + 'static, D: Fn() + 'static>(&self, existing_value: &str, onchange: F, ondone: D) -> Self::DrawResult;
    fn draw_text_input_with_label<F: Fn(&str) -> () + 'static, D: Fn() + 'static>(&self, label: &str, existing_value: &str, onchange: F, ondone: D) -> Self::DrawResult;
    fn draw_multiline_text_input_with_label<F: Fn(&str) -> () + 'static>(&self, label: &str, existing_value: &str, onchange: F) -> Self::DrawResult;
    fn draw_combo_box_with_label<F: Fn(i32) -> () + 'static>(&self, label: &str, current_item: i32, items: &[&str], onchange: F) -> Self::DrawResult;
    fn draw_all_on_same_line(&self, draw_fns: Vec<&Fn() -> Self::DrawResult>) -> Self::DrawResult;
    fn draw_border_around(&self, draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_statusbar(&self, draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_main_menu_bar(&self, draw_menus: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_menu(&self, label: &str, draw_menu_items: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_menu_item<F: Fn() + 'static>(&self, label: &str, onselect: F) -> Self::DrawResult;
    fn focused(&self, draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
}

pub struct Renderer<'a, T> {
    ui_toolkit: &'a mut T,
    controller: Rc<RefCell<Controller>>,
}

impl<'a, T: UiToolkit> Renderer<'a, T> {
    pub fn new(ui_toolkit: &'a mut T, controller: Rc<RefCell<Controller>>) -> Renderer<'a, T> {
        Self {
            ui_toolkit: ui_toolkit,
            controller: Rc::clone(&controller)
        }
    }

    pub fn render_app(&self) -> T::DrawResult {
        self.ui_toolkit.draw_all(vec![
            self.render_main_menu_bar(),
            self.render_code_window(),
            self.render_console_window(),
            self.render_error_window(),
            self.render_edit_pyfuncs(),
            self.render_status_bar()
        ])
    }

    fn render_main_menu_bar(&self) -> T::DrawResult {
        self.ui_toolkit.draw_main_menu_bar(&|| {
            self.ui_toolkit.draw_menu(
                "File",
                &|| {
                    self.ui_toolkit.draw_menu_item("Debug", || {
                        println!("pressed biatch");
                    })
                }
            )
        })
    }

    fn render_edit_pyfuncs(&self) -> T::DrawResult {
        let funcs = self.controller.borrow().execution_environment.list_functions();
        let pyfuncs = funcs.iter()
            .filter_map(|f| f.as_ref().downcast_ref::<pystuff::PyFunc>());
        self.ui_toolkit.draw_all(pyfuncs.map(|f| self.render_edit_pyfunc(f)).collect())
    }

    fn render_edit_pyfunc(&self, pyfunc: &pystuff::PyFunc) -> T::DrawResult {
        self.ui_toolkit.draw_window(&format!("Edit PyFunc: {}", pyfunc.id), &|| {
            let cont1 = Rc::clone(&self.controller);
            let pyfunc1 = pyfunc.clone();
            let cont2 = Rc::clone(&self.controller);
            let pyfunc2 = pyfunc.clone();
            let cont3 = Rc::clone(&self.controller);
            let pyfunc3 = pyfunc.clone();

            self.ui_toolkit.draw_all(vec![
                self.ui_toolkit.draw_text_input_with_label(
                    "Function name",
                    pyfunc.name(),
                    move |newvalue| {
                        let mut pyfunc1 = pyfunc1.clone();
                        pyfunc1.name = newvalue.to_string();
                        cont1.borrow_mut().execution_environment.add_function(Box::new(pyfunc1));
                    },
                    || {},
                ),
                self.ui_toolkit.draw_multiline_text_input_with_label(
                    // TODO: add help text here
                    "Prelude",
                    &pyfunc.prelude,
                    move |newvalue| {
                        let mut pyfunc2 = pyfunc2.clone();
                        pyfunc2.prelude = newvalue.to_string();
                        cont2.borrow_mut().execution_environment.add_function(Box::new(pyfunc2));
                    },
                ),
                self.ui_toolkit.draw_multiline_text_input_with_label(
                    "Code",
                    &pyfunc.eval,
                    move |newvalue| {
                        let mut pyfunc3 = pyfunc3.clone();
                        pyfunc3.eval = newvalue.to_string();
                        cont3.borrow_mut().execution_environment.add_function(Box::new(pyfunc3));
                    },
                ),
                self.render_return_type_selector(&pyfunc),
                self.ui_toolkit.draw_separator(),
                self.render_pyfunc_test_section(pyfunc.clone()),
            ])
        },
        |_|{})
    }

    fn render_return_type_selector(&self, pyfunc: &pystuff::PyFunc) -> T::DrawResult {
        let return_type = pyfunc.returns();
        let currently_selected_type_index = self.controller.borrow().typespecs().iter()
            .position(|t| return_type.matches_spec(t))
            .unwrap();
        let typespec_names = self.controller.borrow().typespec_names();
        let typespec_names : Vec<&str>= typespec_names.iter().map(|s| s as &str)
            .collect();

        let cont = Rc::clone(&self.controller);
        let pyfunc2 = pyfunc.clone();
        self.ui_toolkit.draw_all(vec![
            self.ui_toolkit.draw_combo_box_with_label(
                "Return type",
                currently_selected_type_index as i32,
                &typespec_names,
                move |i| {
                    let new_typespec = cont.borrow().typespecs()[i as usize].clone();
                    cont.borrow_mut().set_typespec(&pyfunc2, &new_typespec, &vec![])
                }
            ),
            self.render_type_params_selector(pyfunc, &vec![])
        ])
    }

    fn render_type_params_selector(&self, pyfunc: &pystuff::PyFunc, nesting_level: &Vec<usize>) -> T::DrawResult {
        let mut return_type = pyfunc.returns();
        let mut return_type = &mut return_type;

        for param_index in nesting_level.into_iter() {
            return_type = &mut return_type.params[*param_index]
        }

        let indent = iter::repeat("\t").take(nesting_level.len()).join("");
        let typespec_names : Vec<String> = self.controller.borrow().typespec_names().iter()
            .map(|typespec_name| format!("\t{}{}", indent, typespec_name)).collect();
        let typespec_names: Vec<&str> = typespec_names.iter()
            .map(|s| s as &str)
            .collect();

        let mut drawn = vec![];

        for (i, param) in return_type.params.iter().enumerate() {
            let typespec = &param.typespec;
            let selected_ts_index = self.controller.borrow()
                .typespec_by_id.get_full(&typespec.id).unwrap().0;
            let mut new_nesting_level = nesting_level.clone();
            new_nesting_level.push(i);

            let cont = Rc::clone(&self.controller);
            let pyfunc1 = pyfunc.clone();
            let nl = new_nesting_level.clone();
            drawn.push(self.ui_toolkit.draw_combo_box_with_label(
                &format!("##{:?}", nl),
                selected_ts_index as i32,
                &typespec_names,
                move|i|{
                    let new_typespec = cont.borrow().typespecs()[i as usize].clone();
                    cont.borrow_mut().set_typespec(&pyfunc1, &new_typespec, &nl);
                }
            ));
            drawn.push(self.render_type_params_selector(&pyfunc, &new_nesting_level));
        }

        self.ui_toolkit.draw_all(drawn)
    }

    fn render_pyfunc_test_section(&self, pyfunc: pystuff::PyFunc) -> T::DrawResult {
        let test_result = self.controller.borrow_mut().get_test_result(&pyfunc);
        let cont = Rc::clone(&self.controller);
        self.ui_toolkit.draw_all(vec![
            self.ui_toolkit.draw_text(&format!("Test result: {}", test_result)),
            self.ui_toolkit.draw_button("Run", GREY_COLOR, move || {
                cont.borrow_mut().run_test(&pyfunc)
            })
        ])
    }

    fn render_status_bar(&self) -> T::DrawResult {
        self.ui_toolkit.draw_statusbar(&|| {
            if let(Some(node)) = self.controller.borrow().get_selected_node() {
                self.ui_toolkit.draw_text(
                    &format!("SELECTED: {}", node.description())
                )
            } else {
                self.ui_toolkit.draw_all(vec![])
            }
        })
    }

    fn render_console_window(&self) -> T::DrawResult {
        let controller = self.controller.clone();
        self.ui_toolkit.draw_window("Console", &|| {
            self.ui_toolkit.draw_text_box(controller.borrow().read_console())
        },
        |_|{})
    }

    fn render_error_window(&self) -> T::DrawResult {
        let controller = self.controller.clone();
        self.ui_toolkit.draw_window("Errors", &|| {
            self.ui_toolkit.draw_text_box(controller.borrow().read_error_console())
        },
        |_|{})
    }

    fn render_code_window(&self) -> T::DrawResult {
        let cont = Rc::clone(&self.controller);

        let loaded_code = self.controller.borrow().loaded_code.clone();
        match loaded_code {
            None => {
                self.ui_toolkit.draw_button("No code loaded", CLEAR_COLOR, &||{})
            },
            Some(ref code) => {
                self.ui_toolkit.draw_window(&code.description(), &|| {
                    self.ui_toolkit.draw_layout_with_bottom_bar(
                        &||{ self.render_code(code) },
                        &||{ self.render_run_button(code) }
                    )},
                    move |keypress| {
                        let mut controller = cont.borrow_mut();
                        controller.handle_keypress_in_code_window(keypress)
                    })
            }
        }
    }

    fn render_code(&self, code_node: &CodeNode) -> T::DrawResult {
        if self.is_editing(code_node) {
            return self.draw_inline_editor(code_node)
        }
        let draw = ||{
            match code_node {
                CodeNode::FunctionCall(function_call) => {
                    self.render_function_call(&function_call)
                }
                CodeNode::StringLiteral(string_literal) => {
                    self.render_string_literal(&string_literal)
                }
                CodeNode::Assignment(assignment) => {
                    self.render_assignment(&assignment)
                }
                CodeNode::Block(block) => {
                    self.render_block(&block)
                }
                CodeNode::VariableReference(variable_reference) => {
                    self.render_variable_reference(&variable_reference)
                }
                CodeNode::FunctionDefinition(function_definition) => {
                    self.ui_toolkit.draw_button(
                        &"Function defs are unimplemented",
                        RED_COLOR,
                        ||{}
                    )
                }
                CodeNode::FunctionReference(function_reference) => {
                    self.render_function_reference(&function_reference)
                }
                CodeNode::Argument(argument) => {
                    self.render_function_call_argument(&argument)
                }
                CodeNode::Placeholder(placeholder) => {
                    self.render_placeholder(&placeholder)
                }
            }
        };

        if self.is_selected(code_node) {
            self.ui_toolkit.draw_border_around(&draw)
        } else {
            self.draw_code_node_and_insertion_point_if_before_or_after(code_node, &draw)
        }
    }

    fn draw_code_node_and_insertion_point_if_before_or_after(&self, code_node: &CodeNode, draw: &Fn() -> T::DrawResult) -> T::DrawResult {
        let mut drawn: Vec<T::DrawResult> = vec![];
        if self.is_insertion_pointer_immediately_before(code_node.id()) {
            drawn.push(self.render_insert_code_node())
        }
        drawn.push(draw());
        if self.is_insertion_pointer_immediately_after(code_node.id()) {
            drawn.push(self.render_insert_code_node())
        }
        self.ui_toolkit.draw_all(drawn)
    }

    fn is_insertion_pointer_immediately_before(&self, id: ID) -> bool {
        let insertion_point = self.controller.borrow().insertion_point();
        match insertion_point {
            Some(InsertionPoint::Before(code_node_id)) if code_node_id == id => {
                true
            }
            _ => false
        }
    }

    fn is_insertion_pointer_immediately_after(&self, id: ID) -> bool {
        let insertion_point = self.controller.borrow().insertion_point();
        match insertion_point {
            Some(InsertionPoint::After(code_node_id)) if code_node_id == id => {
                true
            }
            _ => false
        }
    }

    fn render_insert_code_node(&self) -> T::DrawResult {
        let menu = self.controller.borrow().insert_code_menu.as_ref().unwrap().clone();

        self.ui_toolkit.draw_all(vec![
            self.ui_toolkit.focused(&||{
                let controller_1 = Rc::clone(&self.controller);
                let controller_2 = Rc::clone(&self.controller);
                let insertion_point = menu.insertion_point.clone();
                let new_code_node = menu.selected_option_code();

                self.ui_toolkit.draw_text_input(
                    "",
                    move |input|{
                        controller_1.borrow_mut().insert_code_menu.as_mut()
                            .map(|m| {
                                m.set_search_str(input)
                            });
                    },
                    move ||{
                        let mut controller = controller_2.borrow_mut();
                        if let(Some(ref new_code_node)) = new_code_node {
                            let id = new_code_node.id();
                            controller.hide_insert_code_menu();
                            controller.insert_code(new_code_node.clone(), insertion_point);
                        } else {
                            controller.hide_insert_code_menu();
                        }
                    })
            }),
            self.render_insertion_options(&menu)
        ])
    }

    fn render_insertion_options(&self, menu: &InsertCodeMenu) -> <T as UiToolkit>::DrawResult {
        let options = menu.list_options();
        let render_insertion_options : Vec<Box<Fn() -> T::DrawResult>> = options.iter()
            .map(|option| {
                let c : Box<Fn() -> T::DrawResult> = Box::new(move || {
                    self.render_insertion_option(option, menu.insertion_point)
                });
                c
            })
            .collect();
        self.ui_toolkit.draw_all_on_same_line(
            render_insertion_options.iter()
                .map(|c| c.as_ref()).collect()
        )
    }

    fn render_insertion_option(
        &self, option: &'a InsertCodeMenuOption, insertion_point: InsertionPoint) -> T::DrawResult {
        let is_selected = option.is_selected;
        let button_color = if is_selected { RED_COLOR } else { BLACK_COLOR };
        let controller = Rc::clone(&self.controller);
        let new_code_node = Rc::new(option.new_node.clone());
        let draw = move|| {
            let cont = controller.clone();
            let ncn = new_code_node.clone();
            self.ui_toolkit.draw_small_button(&option.label, button_color, move|| {
                let mut cont2 = cont.borrow_mut();
                cont2.hide_insert_code_menu();
                cont2.insert_code((*ncn).clone(), insertion_point);
            })
        };
        if is_selected {
            self.ui_toolkit.draw_border_around(&draw)
        } else {
            draw()
        }
    }

    fn render_assignment(&self, assignment: &Assignment) -> T::DrawResult {
        self.ui_toolkit.draw_all_on_same_line(vec![
            &|| {
                self.render_inline_editable_button(
                    &assignment.name,
                    PURPLE_COLOR,
                    &CodeNode::Assignment(assignment.clone())
                )
            },
            &|| { self.ui_toolkit.draw_button("=", CLEAR_COLOR, &|| {}) },
            &|| { self.render_code(assignment.expression.as_ref()) }
        ])
    }

    fn render_variable_reference(&self, variable_reference: &VariableReference) -> T::DrawResult {
        let mut controller = self.controller.borrow_mut();
        let loaded_code = controller.loaded_code.as_ref().unwrap();
        let assignment = loaded_code.find_node(variable_reference.assignment_id);
        if let(Some(CodeNode::Assignment(assignment))) = assignment {
            self.ui_toolkit.draw_button(&assignment.name, PURPLE_COLOR, &|| {})
        } else {
            self.ui_toolkit.draw_button("Variable reference not found", RED_COLOR, &|| {})
        }
    }

    fn render_block(&self, block: &Block) -> T::DrawResult {
        self.ui_toolkit.draw_all(
            block.expressions.iter().map(|code| self.render_code(code)).collect())
    }

    fn render_function_call(&self, function_call: &FunctionCall) -> T::DrawResult {
        let render_function_reference_fn = || {
            self.render_code(&function_call.function_reference)
        };

        let mut renderers : Vec<Box<Fn() -> T::DrawResult>> = vec![Box::new(render_function_reference_fn)];
        renderers.push(Box::new(|| {
            self.render_function_call_arguments(
                function_call.function_reference().function_id,
                function_call.args())}));
        self.ui_toolkit.draw_all_on_same_line(
            renderers.iter()
                .map(|b| b.as_ref())
                .collect())
    }

    fn render_function_reference(&self, function_reference: &FunctionReference) -> T::DrawResult {
        let function_id = function_reference.function_id;

        // TODO: don't do validation in here. this is just so i can see what this error looks
        // like visually. for realz, i would probably be better off having a separate validation
        // step. and THEN show the errors in here. or maybe overlay something on the codenode that
        // contains the error
        //
        // UPDATE: so i tried that, but figured i still needed to have this code here. i guess maybe
        // there's gonna be no avoiding doing double validation in some situations, and that's ok
        // i think
        let mut color = RED_COLOR;
        let mut function_name = format!("Error: function ID {} not found", function_id);

        if let(Some(function)) = self.controller.borrow_mut().find_function(function_id) {
            color = BLUE_COLOR;
            function_name = function.name().to_string();
        }
        self.ui_toolkit.draw_button(&function_name, color, &|| {})
    }

    fn render_function_call_arguments(&self, function_id: ID, args: Vec<&lang::Argument>) -> T::DrawResult {
        let function = self.controller.borrow().find_function(function_id)
            .map(|func| func.clone());
        match function {
            Some(function) => {
                self.render_args_for_found_function(&*function, args)
            },
            None => {
                self.render_args_for_missing_function(args)
            }
        }
    }

    fn render_function_call_argument(&self, argument: &lang::Argument) -> T::DrawResult {
        let type_symbol;
        {
            let controller = self.controller.borrow();
            let genie = controller.code_genie().unwrap();
            type_symbol = match genie.get_type_for_arg(argument.argument_definition_id) {
                Some(arg_type) => arg_type.symbol(),
                None => "\u{f059}".to_string(),
            };
        }
        self.ui_toolkit.draw_all_on_same_line(vec![
            &|| {
                let cont2 = Rc::clone(&self.controller);
                let node_id_to_select = argument.id;
                self.ui_toolkit.draw_button(&type_symbol, BLACK_COLOR, move ||{
                    let mut controller = cont2.borrow_mut();
                    controller.mark_as_editing(node_id_to_select);
                })
            },
            &|| {
                self.render_code(argument.expr.as_ref())
            },
        ])
    }

    fn render_args_for_found_function(&self, function: &Function, args: Vec<&lang::Argument>) -> T::DrawResult {
        let provided_arg_by_definition_id : HashMap<ID,lang::Argument> = args.into_iter()
            .map(|arg| (arg.argument_definition_id, arg.clone())).collect();
        let expected_args = function.takes_args();

        let draw_results = expected_args.iter().map(|expected_arg| {
            // TODO: display the argument name somewhere in here?
            if let(Some(provided_arg)) = provided_arg_by_definition_id.get(&expected_arg.id) {
                self.render_code(&CodeNode::Argument(provided_arg.clone()))
            } else {
                self.render_missing_function_argument(expected_arg)
            }
        }).collect();

        // TODO: implement this
        // UHHH: what was this^ TODO for?
        self.ui_toolkit.draw_all(draw_results)
    }

    fn render_missing_function_argument(&self, arg: &lang::ArgumentDefinition) -> T::DrawResult {
        self.ui_toolkit.draw_button(
            "this shouldn't have happened, you've got a missing function arg somehow",
            RED_COLOR,
            &|| {})
    }

    fn render_placeholder(&self, placeholder: &lang::Placeholder) -> T::DrawResult {
        let mut r = YELLOW_COLOR;
        // LOL: mess around w/ some transparency
        r[3] = 0.4;
        // TODO: maybe use the traffic cone instead of the exclamation triangle,
        // which is kinda hard to see
        self.ui_toolkit.draw_button(
            &format!("{} {}", PLACEHOLDER_ICON, placeholder.description),
            r,
            &|| {})
    }

    fn render_args_for_missing_function(&self, args: Vec<&lang::Argument>) -> T::DrawResult {
        // TODO: implement this
        self.ui_toolkit.draw_all(vec![])
    }

    fn render_string_literal(&self, string_literal: &StringLiteral) -> T::DrawResult {
        self.render_inline_editable_button(
            &format!("\u{F10D} {} \u{F10E}", string_literal.value),
            CLEAR_COLOR,
            &CodeNode::StringLiteral(string_literal.clone())
        )
    }

    fn render_run_button(&self, code_node: &CodeNode) -> T::DrawResult {
        let controller = self.controller.clone();
        let code_node = code_node.clone();
        self.ui_toolkit.draw_button("Run", GREY_COLOR, move ||{
            let mut controller = controller.borrow_mut();
            controller.run(&code_node);
        })
    }

    fn render_inline_editable_button(&self, label: &str, color: Color, code_node: &CodeNode) -> T::DrawResult {
        let controller = self.controller.clone();
        let id = code_node.id();
        self.ui_toolkit.draw_button(label, color, move || {
            let mut controller = controller.borrow_mut();
            controller.mark_as_editing(id);
        })
    }

    fn is_selected(&self, code_node: &CodeNode) -> bool {
        Some(code_node.id()) == *self.controller.borrow().get_selected_node_id()
    }

    fn is_editing(&self, code_node: &CodeNode) -> bool {
        self.is_selected(code_node) && self.controller.borrow().editing
    }

    fn draw_inline_editor(&self, code_node: &CodeNode) -> T::DrawResult {
        // this is kind of a mess. render_insert_code_node() does `focus` inside of
        // it. the other parts of the branch need to be wrapped in focus() but not
        // render_insert_code_node()
        match code_node {
            CodeNode::StringLiteral(string_literal) => {
                self.ui_toolkit.focused(&move ||{
                    let new_literal = string_literal.clone();
                    self.draw_inline_text_editor(
                        &string_literal.value,
                        move |new_value| {
                            let mut sl = new_literal.clone();
                            sl.value = new_value.to_string();
                            CodeNode::StringLiteral(sl)
                        })
                })
            },
            CodeNode::Assignment(assignment) => {
                self.ui_toolkit.focused(&|| {
                    let a = assignment.clone();
                    self.draw_inline_text_editor(
                        &assignment.name,
                        move |new_value| {
                            let mut new_assignment = a.clone();
                            new_assignment.name = new_value.to_string();
                            CodeNode::Assignment(new_assignment)
                        })
                })
            },
            CodeNode::Argument(argument) => {
                self.render_insert_code_node()
            }
            _ => {
                self.controller.borrow_mut().editing = false;
                self.ui_toolkit.draw_button(&format!("Not possible to edit {:?}", code_node), RED_COLOR, &||{})
            }
        }
    }

    fn draw_inline_text_editor<F: Fn(&str) -> CodeNode + 'static>(&self, initial_value: &str, new_node_fn: F) -> T::DrawResult {
        let controller = Rc::clone(&self.controller);
        let controller2 = Rc::clone(&self.controller);
        self.ui_toolkit.draw_text_input(
            initial_value,
            move |new_value| {
                let new_node = new_node_fn(new_value);
                controller.borrow_mut().loaded_code.as_mut().unwrap().replace(&new_node)
            },
            move || {
                controller2.borrow_mut().editing = false;
                let loaded_code = controller2.borrow_mut().loaded_code.clone().unwrap();
                let selected_node_id = controller2.borrow().selected_node_id;
                controller2.borrow_mut().mutation_master.log_new_mutation(
                    loaded_code, selected_node_id);
            }
        )
    }
}

#[derive(Clone)]
struct UndoHistoryCell {
    code_node: CodeNode,
    cursor_position: Option<ID>,
}

struct MutationMaster {
    history: RefCell<Vec<UndoHistoryCell>>,
    current_index: RefCell<isize>,
}

impl MutationMaster {
    fn new() -> Self {
        MutationMaster { history: RefCell::new(vec![]), current_index: RefCell::new(-1) }
    }

    pub fn seed_initial_history(&self, code_node: &CodeNode) {
        self.history.borrow_mut().clear();
        *self.current_index.borrow_mut() = -1;
        self.log_new_mutation(code_node.clone(), None);
    }

    fn insert_code(
        &self, node_to_insert: CodeNode, insertion_point: InsertionPoint, genie: &CodeGenie,
        cursor_position: Option<ID>,
    ) -> CodeNode {
        let parent = genie.find_parent(insertion_point.node_id());
        if parent.is_none() {
            panic!("unable to insert new code, couldn't find parent to insert into")

        }
        let parent = parent.unwrap();
        match insertion_point {
            InsertionPoint::Before(_) | InsertionPoint::After(_) => {
                self.insert_new_expression_in_block(
                    node_to_insert, insertion_point, parent.clone(), genie,
                    cursor_position)
            }
            InsertionPoint::Argument(argument_id) => {
                self.insert_expression_into_argument(
                    node_to_insert, argument_id, genie, cursor_position)
            }
        }
    }

    fn insert_expression_into_argument(&self, code_node: CodeNode, argument_id: ID,
                                       genie: &CodeGenie, cursor_position: Option<ID>) -> CodeNode {
        let mut argument = genie.find_node(argument_id).unwrap().into_argument().clone();
        argument.expr = Box::new(code_node);
        let mut root = genie.root().clone();
        root.replace(&CodeNode::Argument(argument));
        self.log_new_mutation(root, cursor_position)
    }

    fn insert_new_expression_in_block(&self, code_node: CodeNode, insertion_point: InsertionPoint,
                                      parent: CodeNode, genie: &CodeGenie, cursor_position: Option<ID>) -> CodeNode {
        match parent {
            CodeNode::Block(mut block) => {
                let insertion_point_in_block_exprs = block.expressions.iter()
                    .position(|exp| exp.id() == insertion_point.node_id());
                if insertion_point_in_block_exprs.is_none() {
                    panic!("when the fuck does this happen?")
                }
                let insertion_point_in_block_exprs = insertion_point_in_block_exprs.unwrap();

                match insertion_point {
                    InsertionPoint::Before(_) => {
                        block.expressions.insert(insertion_point_in_block_exprs, code_node)
                    },
                    InsertionPoint::After(_) => {
                        block.expressions.insert(insertion_point_in_block_exprs + 1, code_node)
                    },
                    _ => panic!("bad insertion point type for a block: {:?}", insertion_point)
                }

                let mut root = genie.root().clone();
                root.replace(&CodeNode::Block(block));
                self.log_new_mutation(root, cursor_position)
            },
            _ => panic!("should be inserting into type parent, got {:?} instead", parent)
        }
    }

    pub fn delete_code(&self, node_to_delete: &CodeNode, genie: &CodeGenie,
                       cursor_position: Option<ID>) -> CodeNode {
        let parent = genie.find_parent(node_to_delete.id());
        if parent.is_none() {
            panic!("idk when this happens, let's take care of this if / when it does")
        }
        let parent = parent.unwrap();
        match parent {
            CodeNode::Block(block) => {
                let mut new_block = block.clone();
                new_block.expressions.retain(|exp| exp.id() != node_to_delete.id());
                let mut new_root = genie.root().clone();
                new_root.replace(&CodeNode::Block(new_block));
                self.log_new_mutation(new_root, cursor_position)
            }
            _ => {
                genie.root().clone()
            }
        }
    }

    fn log_new_mutation(&self, new_root: CodeNode, cursor_position: Option<ID>) -> CodeNode {
        // delete everything after the current index
        self.history.borrow_mut().truncate(((*self.current_index.borrow() + 1) as usize));
        self.history.borrow_mut().push(
            UndoHistoryCell {
                code_node: new_root.clone(),
                cursor_position,
            });
        let mut i = self.current_index.borrow_mut();
        *i = (self.history.borrow().len() - 1) as isize;
        new_root
    }

    pub fn undo(&self) -> Option<UndoHistoryCell> {
        let mut i = self.current_index.borrow_mut();
        if *i <= 0 {
            return None
        }
        *i -= 1;
        // after moving the current index back, if we're still at a valid value, then there's still
        // history we can go back to
        if *i >= 0 {
            self.history.borrow().get(*i as usize).cloned()
        } else {
            None
        }
    }

    pub fn redo(&self) -> Option<UndoHistoryCell> {
        let mut i = self.current_index.borrow_mut();
        if *i == (self.history.borrow().len() - 1) as isize {
            return None
        }
        *i += 1;
        self.history.borrow().get(*i as usize).cloned()
    }
}

// return value: (CodeNode ID, editing: true/false)
fn post_insertion_cursor(code_node: &CodeNode) -> (ID, bool) {
    match code_node {
        CodeNode::FunctionCall(function_call) => {
            if function_call.args.len() > 0 {
                (function_call.args.get(0).unwrap().id(), true)
            } else {
                (function_call.id, false)
            }
        }
        _ => {
            (code_node.id(), false)
        }
    }
}