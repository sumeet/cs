use std::cell::RefCell;
//use debug_cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;
use std::iter;

use objekt::{clone_trait_object,__internal_clone_trait_object};
use failure::{err_msg};
use failure::Error as Error;
use super::env::{ExecutionEnvironment};
use super::lang;
use super::code_loading;
use super::code_generation;
use super::lang::{
    Value,CodeNode,Function,FunctionCall,FunctionReference,StringLiteral,ID,Assignment,Block,
    VariableReference};
use itertools::Itertools;
use super::pystuff;
use super::jsstuff;
use super::external_func;
use super::undo;
use super::edit_types;
use super::structs;


pub const SELECTION_COLOR: Color = [1., 1., 1., 0.3];
pub const BLUE_COLOR: Color = [100.0 / 255.0, 149.0 / 255.0, 237.0 / 255.0, 1.0];
pub const YELLOW_COLOR: Color = [253.0 / 255.0, 159.0 / 255.0, 19.0 / 255.0, 1.0];
pub const BLACK_COLOR: Color = [0.0, 0.0, 0.0, 1.0];
pub const RED_COLOR: Color = [0.858, 0.180, 0.180, 1.0];
pub const GREY_COLOR: Color = [0.521, 0.521, 0.521, 1.0];
pub const PURPLE_COLOR: Color = [0.486, 0.353, 0.952, 1.0];
pub const CLEAR_COLOR: Color = [0.0, 0.0, 0.0, 0.0];
pub const SALMON_COLOR: Color = [0.996, 0.286, 0.322, 1.0];

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
    selected_option_index: isize,
    search_params: CodeSearchParams,
    insertion_point: InsertionPoint,
}

// TODO: could probably have a single interface for generating this menu, inject the insertion point
// into the search params (because we can copy it easily) and then we don't need to box the searchers.
// every searcher can decide for itself if it can fill in that particular insertion point
impl InsertCodeMenu {
    // TODO: we could just construct all the generators and have them switch on insertion
    // point, intead of doing it here... maybe
    pub fn for_insertion_point(insertion_point: InsertionPoint, genie: &CodeGenie) -> Self {
        match insertion_point {
            InsertionPoint::Before(_) | InsertionPoint::After(_) => {
                Self::new_expression_inside_code_block(insertion_point)
            },
            InsertionPoint::Argument(field_id) | InsertionPoint::StructLiteralField(field_id) => {
                Self::fill_in_field_with_exact_type(field_id, genie, insertion_point)
            }
        }
    }

    fn new_expression_inside_code_block(insertion_point: InsertionPoint) -> Self {
        Self {
            // TODO: should probably be able to insert new assignment expressions as well
            option_generators: vec![Box::new(InsertFunctionOptionGenerator {})],
            selected_option_index: 0,
            search_params: CodeSearchParams::empty(),
            insertion_point,
        }
    }

    fn fill_in_field_with_exact_type(id: lang::ID, genie: &CodeGenie,
                                     insertion_point: InsertionPoint) -> Self {
        let node = genie.find_node(id).unwrap();
        let exact_type = genie.guess_type(node);

        Self {
            option_generators: vec![
                Box::new(InsertVariableReferenceOptionGenerator {
                    insertion_id: id
                }),
                Box::new(InsertFunctionOptionGenerator {}),
                Box::new(InsertLiteralOptionGenerator {}),
            ],
            selected_option_index: 0,
            search_params: CodeSearchParams::with_type(&exact_type),
            insertion_point,
        }
    }

    fn selected_option_code(&self, code_genie: &CodeGenie) -> Option<CodeNode> {
        let all_options = self.list_options(code_genie);
        if all_options.is_empty() {
            return None
        }
        let selected_index = self.selected_index(all_options.len());
        Some(all_options.get(selected_index)?.new_node.clone())
    }

    fn select_next(&mut self) {
        // this could possibly overflow, but i wouldn't count on it... HAXXXXX
        self.selected_option_index += 1;
    }

    fn search_str(&self) -> &str {
        &self.search_params.input_str
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
    fn list_options(&self, code_genie: &CodeGenie) -> Vec<InsertCodeMenuOption> {
        let mut all_options : Vec<InsertCodeMenuOption> = self.option_generators
            .iter()
            .flat_map(|generator| {
                generator.options(&self.search_params, code_genie)
            })
            .collect();
        if all_options.is_empty() {
            return all_options
        }
        let selected_index = self.selected_index(all_options.len());
        all_options.get_mut(selected_index).as_mut()
            .map(|option| option.is_selected = true);
        all_options
    }

    // HACK: this modulo stuff is an insane hack but it lets me not have to pass a code genie into
    // select_next
    fn selected_index(&self, num_options: usize) -> usize {
        (self.selected_option_index % num_options as isize) as usize
    }
}

trait InsertCodeMenuOptionGenerator : objekt::Clone {
    fn options(&self, search_params: &CodeSearchParams, code_genie: &CodeGenie) -> Vec<InsertCodeMenuOption>;
}

clone_trait_object!(InsertCodeMenuOptionGenerator);

// TODO: should the insertion point go inside here as well? that way we wouldn't have to store off
// the ID in the variable reference searcher
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

    pub fn lowercased_trimmed_search_str(&self) -> String {
        self.input_str.trim().to_lowercase()
    }
}

#[derive(Clone)]
struct InsertCodeMenuOption {
    label: String,
    new_node: CodeNode,
    is_selected: bool,
}

#[derive(Clone)]
struct InsertFunctionOptionGenerator {}

impl InsertCodeMenuOptionGenerator for InsertFunctionOptionGenerator {
    fn options(&self, search_params: &CodeSearchParams, genie: &CodeGenie) -> Vec<InsertCodeMenuOption> {
        let mut functions : &mut Iterator<Item = &Box<Function>> = &mut genie.all_functions();
        let mut a;
        let mut b;

        let input_str = search_params.lowercased_trimmed_search_str();
        if !input_str.is_empty() {
            a = functions
                .filter(|f| {
                    f.name().to_lowercase().contains(&input_str)
                });
            functions = &mut a;
        }

        let return_type = &search_params.return_type;
        if return_type.is_some() {
            b = functions
                .filter(|f| f.returns().matches(return_type.as_ref().unwrap()));
            functions = &mut b;
        }

        functions.map(|func| {
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
    insertion_id: lang::ID,
}

impl InsertCodeMenuOptionGenerator for InsertVariableReferenceOptionGenerator {
    fn options(&self, search_params: &CodeSearchParams,
               genie: &CodeGenie) -> Vec<InsertCodeMenuOption> {
        let assignments_by_type_id : HashMap<ID, Vec<lang::Assignment>> = genie.find_assignments_that_come_before_code(self.insertion_id)
            .into_iter()
            .group_by(|assignment| {
                let assignment : Assignment = (**assignment).clone();
                genie.guess_type(&CodeNode::Assignment(assignment)).id()
            })
            .into_iter()
            .map(|(id, assignments)| (id, assignments.cloned().collect::<Vec<Assignment>>()))
            .collect();

        let mut assignments = if let Some(search_type) = &search_params.return_type {
            // XXX: this won't work for generics i believe
            assignments_by_type_id.get(&search_type.id()).map_or_else(
                || vec![],
                |assignments| assignments.iter()
                    .map(|assignment| assignment.clone()).collect()
            )
        } else {
            assignments_by_type_id.iter()
                .flat_map(|(_id, assignments)| assignments)
                .map(|assignment| assignment.clone())
                .collect()
        };

        let input_str = search_params.lowercased_trimmed_search_str();
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

// TODO: some literals we'll want to construct anywhere... you can put something down even if it's
// not evaluated and just wrap it later!
//impl InsertLiteralOptionGenerator {
//    fn maybe_insert_string_literal(&self, options: &mut Vec<InsertCodeMenuOption>,
//                                   search_params: &CodeSearchParams) {
//        let input_str = &search_params.input_str;
//    }
//}

impl InsertCodeMenuOptionGenerator for InsertLiteralOptionGenerator {
    fn options(&self, search_params: &CodeSearchParams, genie: &CodeGenie) -> Vec<InsertCodeMenuOption> {
        let mut options = vec![];
        let input_str = &search_params.input_str;
        if let Some(ref return_type) = search_params.return_type {
            if return_type.matches_spec(&lang::STRING_TYPESPEC) {
                options.push(
                    InsertCodeMenuOption {
                        label: format!("\u{f10d}{}\u{f10e}", input_str),
                        is_selected: false,
                        new_node: code_generation::new_string_literal(input_str)
                    }
                );
            } else if return_type.matches_spec(&lang::NULL_TYPESPEC) {
                options.push(
                    InsertCodeMenuOption {
                        label: lang::NULL_TYPESPEC.symbol.clone(),
                        is_selected: false,
                        new_node: code_generation::new_null_literal(),
                    }
                );
            } else if let Some(strukt) = genie.find_struct(return_type.typespec_id) {
                options.push(
                    InsertCodeMenuOption {
                        label: format!("{} {}", strukt.symbol, strukt.name),
                        is_selected: false,
                        new_node: code_generation::new_struct_literal_with_placeholders(strukt),
                    }
                );
            }

            // design decision made here: all placeholders have types. therefore, it is now
            // required for a placeholder node to have a type, meaning we need to know what the
            // type of a placeholder is to create it. under current conditions that's ok, but i
            // think we can make this less restrictive in the future if we need to
            options.push(
                InsertCodeMenuOption {
                    label: format!("{} {}", PLACEHOLDER_ICON, input_str),
                    is_selected: false,
                    new_node: code_generation::new_placeholder(input_str, return_type.id()),
                }
            );
        }
        options
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InsertionPoint {
    Before(ID),
    After(ID),
    Argument(ID),
    StructLiteralField(ID),
}

impl InsertionPoint {
    fn node_id(&self) -> ID {
        match *self {
            InsertionPoint::Before(id) => id,
            InsertionPoint::After(id) => id,
            InsertionPoint::Argument(id) => id,
            InsertionPoint::StructLiteralField(id) => id,
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
    J,
    K,
    L,
    W,
    X,
    R,
    O,
    U,
    V,
    Tab,
    Escape,
    UpArrow,
    DownArrow,
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

    // TODO: bug??? for when we add conditionals, it's possible this won't detect assignments made
    // inside of conditionals... ugh scoping is tough
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
                let position_in_block = self.find_position_in_block(&block, block_expression_id).unwrap();
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

    fn find_position_in_block(&self, block: &lang::Block, block_expression_id: ID) -> Option<usize> {
        block.expressions.iter().position(|code| code.id() == block_expression_id)
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

    fn all_functions(&self) -> impl Iterator<Item = &Box<lang::Function>> {
        self.env.list_functions()
    }

    fn find_struct(&self, id: lang::ID) -> Option<&structs::Struct> {
        self.env.find_struct(id)
    }

    pub fn guess_type(&self, code_node: &CodeNode) -> lang::Type {
        match code_node {
            CodeNode::FunctionCall(function_call) => {
                let func_id = function_call.function_reference().function_id;
                match self.find_function(func_id) {
                    Some(ref func) => func.returns().clone(),
                    // TODO: do we really want to just return Null if we couldn't find the function?
                    None => lang::Type::from_spec(&*lang::NULL_TYPESPEC),
                }
            }
            CodeNode::StringLiteral(_) => {
                lang::Type::from_spec(&*lang::STRING_TYPESPEC)
            }
            CodeNode::Assignment(assignment) => {
                self.guess_type(&*assignment.expression)
            }
            CodeNode::Block(block) => {
                if block.expressions.len() > 0 {
                    let last_expression_in_block= &block.expressions[block.expressions.len() - 1];
                    self.guess_type(last_expression_in_block)
                } else {
                    lang::Type::from_spec(&*lang::NULL_TYPESPEC)
                }
            }
            CodeNode::VariableReference(_) => {
                lang::Type::from_spec(&*lang::NULL_TYPESPEC)
            }
            CodeNode::FunctionReference(_) => {
                lang::Type::from_spec(&*lang::NULL_TYPESPEC)
            }
            CodeNode::FunctionDefinition(_) => {
                lang::Type::from_spec(&*lang::NULL_TYPESPEC)
            }
            CodeNode::Argument(arg) => {
                self.get_type_for_arg(arg.argument_definition_id).unwrap()
            }
            CodeNode::Placeholder(_) => {
                lang::Type::from_spec(&*lang::NULL_TYPESPEC)
            }
            CodeNode::NullLiteral => {
                lang::Type::from_spec(&*lang::NULL_TYPESPEC)
            },
            CodeNode::StructLiteral(struct_literal) => {
                let strukt = self.env.find_struct(struct_literal.struct_id).unwrap();
                lang::Type::from_spec(strukt)
            }
            CodeNode::StructLiteralField(struct_literal_field) => {
                let strukt_literal = self.find_parent(struct_literal_field.id)
                    .unwrap().into_struct_literal().unwrap();
                let strukt = self.env.find_struct(strukt_literal.struct_id).unwrap();
                strukt.field_by_id().get(&struct_literal_field.struct_field_id).unwrap()
                    .field_type.clone()
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

    pub fn navigate_up_from(&self, code_node_id: Option<ID>) -> Option<ID> {
        let code_node_id = code_node_id?;
        let containing_block_expression_id = self.code_genie
            .find_expression_inside_block_that_contains(code_node_id)?;
        let position_inside_block_expression = self.code_genie
            .find_node(containing_block_expression_id)?
            .self_with_all_children_dfs()
            .filter(|cn| self.is_navigatable(cn))
            .position(|child_node| child_node.id() == code_node_id)?;

        let block = self.code_genie.find_parent(containing_block_expression_id)?.into_block()?;
        let position_of_block_expression_inside_block = self.code_genie
            .find_position_in_block(block, containing_block_expression_id)?;
        let previous_block_expression = block.expressions.get(position_of_block_expression_inside_block - 1)?;

        let expressions_in_previous_block_expression_up_to_our_index = previous_block_expression
            .self_with_all_children_dfs()
            .filter(|cn| self.is_navigatable(cn))
            .take(position_inside_block_expression + 1)
            .collect_vec();

        let expression_in_previous_block_expression_with_same_or_latest_index_id =
            expressions_in_previous_block_expression_up_to_our_index.get(position_inside_block_expression)
                .or_else(|| expressions_in_previous_block_expression_up_to_our_index.last())?;
        Some(expression_in_previous_block_expression_with_same_or_latest_index_id.id())
    }

    pub fn navigate_down_from(&self, code_node_id: Option<ID>) -> Option<ID> {
        // if nothing's selected and you try going down, let's just go to the first selectable node
        if code_node_id.is_none() {
            return self.navigate_forward_from(code_node_id)
        }
        let code_node_id = code_node_id.unwrap();
        let containing_block_expression_id = self.code_genie
            .find_expression_inside_block_that_contains(code_node_id)?;
        let position_inside_block_expression = self.code_genie
            .find_node(containing_block_expression_id)?
            .self_with_all_children_dfs()
            .filter(|cn| self.is_navigatable(cn))
            .position(|child_node| child_node.id() == code_node_id)?;

        let block = self.code_genie.find_parent(containing_block_expression_id)?.into_block()?;
        let position_of_block_expression_inside_block = self.code_genie
            .find_position_in_block(block, containing_block_expression_id)?;
        let previous_block_expression = block.expressions.get(position_of_block_expression_inside_block + 1)?;

        let expressions_in_previous_block_expression_up_to_our_index = previous_block_expression
            .self_with_all_children_dfs()
            .filter(|cn| self.is_navigatable(cn))
            .take(position_inside_block_expression + 1)
            .collect_vec();

        let expression_in_previous_block_expression_with_same_or_latest_index_id =
            expressions_in_previous_block_expression_up_to_our_index.get(position_inside_block_expression)
                .or_else(|| expressions_in_previous_block_expression_up_to_our_index.last())?;
        Some(expression_in_previous_block_expression_with_same_or_latest_index_id.id())
    }

    pub fn navigate_back_from(&self, code_node_id: Option<ID>) -> Option<ID> {
        if code_node_id.is_none() {
            return None
        }
        let mut go_back_from_id = code_node_id.unwrap();
        while let Some(prev_node) = self.prev_node_from(go_back_from_id) {
           if self.is_navigatable(prev_node) {
               return Some(prev_node.id())
           } else {
               go_back_from_id = prev_node.id()
           }
        }
        None
    }

    pub fn navigate_forward_from(&self, code_node_id: Option<ID>) -> Option<ID> {
        let mut go_back_from_id = code_node_id;
        while let Some(prev_node) = self.next_node_from(go_back_from_id) {
            if self.is_navigatable(prev_node) {
                return Some(prev_node.id())
            } else {
                go_back_from_id = Some(prev_node.id())
            }
        }
        None
    }


    fn prev_node_from(&self, code_node_id: ID) -> Option<&CodeNode> {
        let parent = self.code_genie.find_parent(code_node_id);
        if parent.is_none() {
            return None
        }
        let parent = parent.unwrap();
        // first try the previous sibling
        if let Some(previous_sibling) = parent.previous_child(code_node_id) {
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

    fn next_node_from(&self, code_node_id: Option<ID>) -> Option<&CodeNode> {
        if code_node_id.is_none() {
            return Some(self.code_genie.root())
        }

        let selected_node_id = code_node_id.unwrap();
        let selected_code = self.code_genie.find_node(selected_node_id).unwrap();
        let children = selected_code.children();
        let first_child = children.get(0);

        // if the selected node has children, then return the first child. depth first
        if let Some(first_child) = first_child {
            return Some(first_child)
        }

        let mut node_id_to_find_next_sibling_of = selected_node_id;
        while let Some(parent) = self.code_genie.find_parent(node_id_to_find_next_sibling_of) {
            if let Some(next_sibling) = parent.next_child(node_id_to_find_next_sibling_of) {
                return Some(next_sibling)
            }
            // if there is no sibling, then try going to the next sibling of the parent, recursively
            node_id_to_find_next_sibling_of = parent.id()
        }
        None
    }

    // navigation entails moving forward and backwards with the cursor, using the keyboard. i'd like
    // for this keyboard based navigation to feel ergonomic, so when you're navigating through items,
    // the cursor doesn't get stuck on elements that you didn't really care to navigate to. therefore
    // i've arrived at the following rules:
    fn is_navigatable(&self, code_node: &CodeNode) -> bool {
        match code_node {
            // skip entire code blocks: you want to navigate individual elements, and entire codeblocks are
            // huge chunks of code
            CodeNode::Block(_) => false,
            // you always want to be able to edit the name of an assignment
            CodeNode::Assignment(_) => true,
            // instead of navigating over the entire function call, you want to navigate through its
            // innards. that is, the function reference (so you can change the function that's being
            // referred to), or the holes (arguments)
            CodeNode::FunctionCall(_) => false,
            CodeNode::FunctionReference(_) => true,
            // skip holes. function args and struct literal fields always contain inner elements
            // that can be changed. to change those, we can always invoke `r` (replace), which will
            // let you edit the value of the hole
            CodeNode::Argument(_) | CodeNode::StructLiteralField(_) => false,
            // you always want to move to literals
            CodeNode::StringLiteral(_) | CodeNode::NullLiteral | CodeNode::StructLiteral(_) => true,
            _ => match self.code_genie.find_parent(code_node.id()) {
                Some(parent) => {
                    match parent {
                        // if our parent is an arg or struct literal field, then we're inside a hole.
                        // let's make ourselves navigatable
                        CodeNode::Argument(_) | CodeNode::StructLiteralField(_) => true,
                        _ => false,
                    }
                }
                None => false
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
            mutation_master: MutationMaster::new(),
            test_result_by_func_id: HashMap::new(),
        }
    }

    fn typespecs(&self) -> impl Iterator<Item = &Box<lang::TypeSpec>> {
        self.execution_environment.list_typespecs()
    }

    fn list_structs(&self) -> impl Iterator<Item = &structs::Struct> {
        self.typespecs()
            .filter_map(|ts| ts.as_ref().downcast_ref::<structs::Struct>())
    }

    fn get_typespec(&self, id: lang::ID) -> Option<&Box<lang::TypeSpec>> {
        self.execution_environment.find_typespec(id)
    }

    fn save(&self) {
        let theworld = code_loading::TheWorld {
            main_code: self.loaded_code.clone().unwrap(),
            pyfuncs: self.list_pyfuncs(),
            jsfuncs: self.list_jsfuncs(),
            structs: self.list_structs().cloned().collect(),
        };
        code_loading::save("codesample.json", &theworld).unwrap();
    }

    fn list_jsfuncs(&self) -> Vec<jsstuff::JSFunc> {
        self.execution_environment.list_functions()
            .filter_map(|f| Result::ok(f.clone().downcast::<jsstuff::JSFunc>()))
            .map(|boxedjsfunc| *boxedjsfunc).collect()
    }

    fn list_pyfuncs(&self) -> Vec<pystuff::PyFunc> {
        self.execution_environment.list_functions()
            .filter_map(|f| Result::ok(f.clone().downcast::<pystuff::PyFunc>()))
            .map(|boxedpyfunc| *boxedpyfunc).collect()
    }

    // TODO: return a result instead of returning nothing? it seems like there might be places this
    // thing can error
    fn insert_code(&mut self, code_node: CodeNode, insertion_point: InsertionPoint) {
        let genie = self.code_genie();
        let new_code = self.mutation_master.insert_code(&code_node, insertion_point,
                                                        genie.as_ref().unwrap());
        self.loaded_code.as_mut().unwrap().replace(&new_code);
        let genie = self.code_genie();
        let (next_selection_id, editing) = post_insertion_cursor(&code_node, genie.as_ref().unwrap());
        if editing {
            self.mark_as_editing(next_selection_id)
        } else {
            self.set_selected_node_id(Some(next_selection_id));
        }
    }

    fn get_test_result(&self, func: &lang::Function) -> String {
        let test_result = self.test_result_by_func_id.get(&func.id());
        if let Some(test_result) = test_result {
            format!("{:?}", test_result.value)
        } else {
            "Test not run yet".to_string()
        }
    }

    fn run_test(&mut self, func: &lang::Function) {
        // XXX lol ghetto
        let fc = code_generation::new_function_call_with_placeholder_args(func);
        let result = TestResult::new(self.run(&fc));
        self.test_result_by_func_id.insert(func.id(), result);
    }

    fn undo(&mut self) {
        if self.loaded_code.is_none() {
            return
        }
        let loaded_code = self.loaded_code.as_ref().unwrap();
        if let Some(previous_root) = self.mutation_master.undo(loaded_code, self.selected_node_id) {
            self.loaded_code.as_mut().unwrap().replace(&previous_root.root);
            self.set_selected_node_id(previous_root.cursor_position);
        }
    }

    fn redo(&mut self) {
        let loaded_code = self.loaded_code.as_ref().unwrap();
        if let Some(next_root) = self.mutation_master.redo(loaded_code, self.selected_node_id) {
            self.loaded_code.as_mut().unwrap().replace(&next_root.root);
            self.set_selected_node_id(next_root.cursor_position);
        }
    }

    fn delete_selected_code(&mut self) {
        let node_to_delete = self.get_selected_node().cloned().unwrap();
        let genie = self.code_genie();
        let deletion_result = self.mutation_master.delete_code(
            &node_to_delete, genie.as_ref().unwrap(), self.selected_node_id);
        // TODO: these save current state calls can go inside of the mutation master
        self.save_current_state_to_undo_history();
        self.loaded_code.as_mut().unwrap().replace(&deletion_result.new_root);
        // TODO: intelligently select a nearby node to select after deleting
        self.set_selected_node_id(deletion_result.new_cursor_position);
    }

    fn select_current_line(&mut self) {
        let genie = self.code_genie();
        if genie.is_none() || self.selected_node_id.is_none() {
            return
        }
        let genie = genie.unwrap();
        let selected_id = self.selected_node_id.unwrap();
        if let Some(code_id) = genie.find_expression_inside_block_that_contains(selected_id) {
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
            (false, Key::K) | (false, Key::UpArrow) => {
                self.try_select_up_one_node()
            },
            (false, Key::J) | (false, Key::DownArrow) => {
                self.try_select_down_one_node()
            },
            (false, Key::B) | (false, Key::LeftArrow) | (false, Key::H) => {
                self.try_select_back_one_node()
            },
            (false, Key::W) | (false, Key::RightArrow) | (false, Key::L) => {
                self.try_select_forward_one_node()
            },
            (false, Key::C) => {
                if let Some(id) = self.selected_node_id {
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
                } else {
                    self.try_enter_replace_edit_for_selected_node();
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
                self.insert_code_menu.as_mut().map(|menu| menu.select_next());
            }
            _ => {},
        }
    }

    fn try_enter_replace_edit_for_selected_node(&mut self) {
        if let Some(id) = self.selected_node_id {
            if let Some(genie) = self.code_genie() {
                if let Some(parent) = genie.find_parent(id) {
                    match parent {
                        CodeNode::Argument(cn) => {
                            self.mark_as_editing(cn.id)
                        },
                        CodeNode::StructLiteralField(cn) => {
                            self.mark_as_editing(cn.id)
                        },
//                        CodeNode::Block(block) => {
//                            self.mark_as_editing(cn.id)
//                        },
                        _ => (),
                    }
                }
            }
        }
    }

    fn handle_cancel(&mut self) {
        self.editing = false;
        if self.insert_code_menu.is_none() { return }
        self.undo();
        self.hide_insert_code_menu()
    }

    // TODO: factor duplicate code between this method and the next
    fn set_insertion_point_on_previous_line_in_block(&mut self) {
        if let Some(expression_id) = self.currently_focused_block_expression() {
            self.save_current_state_to_undo_history();
            self.insert_code_menu = Some(InsertCodeMenu::for_insertion_point(
                InsertionPoint::Before(expression_id),
                self.code_genie().as_ref().unwrap()
            ));
            self.editing = true;
            self.selected_node_id = None;
        } else {
            self.hide_insert_code_menu()
        }
    }

    fn set_insertion_point_on_next_line_in_block(&mut self) {
        if let Some(expression_id) = self.currently_focused_block_expression() {
            self.save_current_state_to_undo_history();
            self.insert_code_menu = Some(InsertCodeMenu::for_insertion_point(
                InsertionPoint::After(expression_id),
                self.code_genie().as_ref().unwrap()
            ));
            self.editing = true;
            self.selected_node_id = None;
        } else {
            self.hide_insert_code_menu()
        }
    }

    fn mark_as_editing(&mut self, node_id: ID) {
        {
            let genie = self.code_genie();
            if genie.is_none() {
                return
            }
        }
        self.save_current_state_to_undo_history();
        let genie = self.code_genie().unwrap();
        match genie.find_node(node_id) {
            Some(CodeNode::Argument(_)) => {
                let insertion_point = InsertionPoint::Argument(node_id);
                self.insert_code_menu = Some(InsertCodeMenu::for_insertion_point(insertion_point, &genie));
            },
            Some(CodeNode::StructLiteralField(_)) => {
                let insertion_point = InsertionPoint::StructLiteralField(node_id);
                self.insert_code_menu = Some(InsertCodeMenu::for_insertion_point(insertion_point, &genie));
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

    pub fn try_select_up_one_node(&mut self) {
        let genie = self.code_genie();
        let navigation = Navigation::new(genie.as_ref().unwrap());
        if let Some(node_id) = navigation.navigate_up_from(self.selected_node_id) {
            self.set_selected_node_id(Some(node_id))
        }
    }

    pub fn try_select_down_one_node(&mut self) {
        let genie = self.code_genie();
        let navigation = Navigation::new(genie.as_ref().unwrap());
        if let Some(node_id) = navigation.navigate_down_from(self.selected_node_id) {
            self.set_selected_node_id(Some(node_id))
        }
    }

    pub fn try_select_back_one_node(&mut self) {
        let genie = self.code_genie();
        let navigation = Navigation::new(genie.as_ref().unwrap());
        if let Some(node_id) = navigation.navigate_back_from(self.selected_node_id) {
            self.set_selected_node_id(Some(node_id))
        }
    }

    pub fn try_select_forward_one_node(&mut self) {
        let genie = self.code_genie();
        let navigation = Navigation::new(genie.as_ref().unwrap());
        if let Some(node_id) = navigation.navigate_forward_from(self.selected_node_id) {
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

    pub fn set_typespec<F: external_func::ModifyableFunc>(&mut self, mut func: F,
                                                          typespec: &Box<lang::TypeSpec>,
                                                          nesting_level: &[usize]) {
        let mut root_return_type = func.returns();
        edit_types::set_typespec(&mut root_return_type, typespec, nesting_level);
        func.set_return_type(root_return_type);
        self.load_function(func)
    }

    pub fn load_typespec<T: lang::TypeSpec + 'static>(&mut self, typespec: T) {
        self.execution_environment.add_typespec(typespec)
    }

    pub fn load_function<F: Function>(&mut self, function: F) {
        self.execution_environment.add_function(Box::new(function))
    }

    pub fn remove_function(&mut self, id: lang::ID) {
        self.execution_environment.delete_function(id)
    }

    pub fn find_function(&self, id: ID) -> Option<&Box<Function>> {
        self.execution_environment.find_function(id)
    }

    pub fn load_code(&mut self, code_node: &CodeNode) {
        self.loaded_code = Some(code_node.clone());
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

    pub fn save_current_state_to_undo_history(&mut self) {
        if let Some(ref loaded_code) = self.loaded_code {
            self.mutation_master.log_new_mutation(loaded_code, self.selected_node_id)
        }
    }
}

pub trait UiToolkit {
    type DrawResult;

    fn draw_all(&self, draw_results: Vec<Self::DrawResult>) -> Self::DrawResult;
    fn draw_window<F: Fn(Keypress) + 'static>(&self, window_name: &str, draw_fn: &Fn() -> Self::DrawResult, handle_keypress: Option<F>) -> Self::DrawResult;
    fn draw_layout_with_bottom_bar(&self, draw_content_fn: &Fn() -> Self::DrawResult, draw_bottom_bar_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_empty_line(&self) -> Self::DrawResult;
    fn draw_separator(&self) -> Self::DrawResult;
    fn draw_text(&self, text: &str) -> Self::DrawResult;
    fn draw_text_with_label(&self, text: &str, label: &str) -> Self::DrawResult;
    fn draw_button<F: Fn() + 'static>(&self, label: &str, color: Color, onclick: F) -> Self::DrawResult;
    fn draw_small_button<F: Fn() + 'static>(&self, label: &str, color: Color, onclick: F) -> Self::DrawResult;
    fn draw_text_box(&self, text: &str) -> Self::DrawResult;
    fn draw_text_input<F: Fn(&str) + 'static, D: Fn() + 'static>(&self, existing_value: &str, onchange: F, ondone: D) -> Self::DrawResult;
    fn draw_text_input_with_label<F: Fn(&str) + 'static, D: Fn() + 'static>(&self, label: &str, existing_value: &str, onchange: F, ondone: D) -> Self::DrawResult;
    fn draw_multiline_text_input_with_label<F: Fn(&str) -> () + 'static>(&self, label: &str, existing_value: &str, onchange: F) -> Self::DrawResult;
    fn draw_combo_box_with_label<F, G, H, T>(&self, label: &str, is_item_selected: G, format_item: H, items: &[&T], onchange: F) -> Self::DrawResult
        where T: Clone + 'static,
              F: Fn(&T) -> () + 'static,
              G: Fn(&T) -> bool,
              H: Fn(&T) -> String;
    fn draw_all_on_same_line(&self, draw_fns: &[&Fn() -> Self::DrawResult]) -> Self::DrawResult;
    fn draw_box_around(&self, color: [f32; 4], draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_top_border_inside(&self, color: [f32; 4], thickness: u8,
                              draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_right_border_inside(&self, color: [f32; 4], thickness: u8,
                                 draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_bottom_border_inside(&self, color: [f32; 4], thickness: u8,
                                 draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_statusbar(&self, draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_main_menu_bar(&self, draw_menus: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_menu(&self, label: &str, draw_menu_items: &Fn() -> Self::DrawResult) -> Self::DrawResult;
    fn draw_menu_item<F: Fn() + 'static>(&self, label: &str, onselect: F) -> Self::DrawResult;
    fn focused(&self, draw_fn: &Fn() -> Self::DrawResult) -> Self::DrawResult;
}

pub struct Renderer<'a, T> {
    arg_nesting_level: RefCell<u32>,
    ui_toolkit: &'a mut T,
    controller: Rc<RefCell<Controller>>,
}

impl<'a, T: UiToolkit> Renderer<'a, T> {
    pub fn new(ui_toolkit: &'a mut T, controller: Rc<RefCell<Controller>>) -> Renderer<'a, T> {
        Self {
            arg_nesting_level: RefCell::new(0),
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
            self.render_edit_jsfuncs(),
            self.render_edit_structs(),
            self.render_status_bar()
        ])
    }

    fn render_main_menu_bar(&self) -> T::DrawResult {
        self.ui_toolkit.draw_main_menu_bar(&|| {
            self.ui_toolkit.draw_menu(
                "File",
                &|| {
                    let cont1 = Rc::clone(&self.controller);
                    #[cfg(feature = "default")]
                    let cont2 = Rc::clone(&self.controller);
                    self.ui_toolkit.draw_all(vec![
                        self.ui_toolkit.draw_menu_item("Save", move || {
                            cont1.borrow_mut().save();
                        }),
                        #[cfg(feature = "default")]
                        self.ui_toolkit.draw_menu_item("Add Python function", move || {
                            cont2.borrow_mut().load_function(pystuff::PyFunc::new());
                        }),
                        self.ui_toolkit.draw_menu_item("Exit", || {
                            std::process::exit(0);
                        }),
                    ])
                }
            )
            // TODO: add a button for creating pyfuncs
            // TODO: add an exit button
        })
    }

    fn render_edit_pyfuncs(&self) -> T::DrawResult {
        let pyfuncs = {
            let controller = self.controller.borrow();
            let funcs = controller.execution_environment.list_functions().cloned();
            funcs.filter_map(|f| f.downcast::<pystuff::PyFunc>().ok()).collect_vec()
        };
        self.ui_toolkit.draw_all(pyfuncs.iter().map(|f| self.render_edit_pyfunc(f)).collect())
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
                        cont1.borrow_mut().load_function(pyfunc1);
                    },
                    || {},
                ),
                self.render_arguments_selector(pyfunc.clone()),
                self.ui_toolkit.draw_multiline_text_input_with_label(
                    // TODO: add help text here
                    "Prelude",
                    &pyfunc.prelude,
                    move |newvalue| {
                        let mut pyfunc2 = pyfunc2.clone();
                        pyfunc2.prelude = newvalue.to_string();
                        cont2.borrow_mut().load_function(pyfunc2);
                    },
                ),
                self.ui_toolkit.draw_multiline_text_input_with_label(
                    "Code",
                    &pyfunc.eval,
                    move |newvalue| {
                        let mut pyfunc3 = pyfunc3.clone();
                        pyfunc3.eval = newvalue.to_string();
                        cont3.borrow_mut().load_function(pyfunc3);
                    },
                ),
                self.render_return_type_selector(pyfunc),
                self.ui_toolkit.draw_separator(),
                self.render_test_section(pyfunc.clone()),
                self.ui_toolkit.draw_separator(),
                self.render_general_function_menu(pyfunc),
            ])
        },
        None::<fn(Keypress)>)
    }

    fn render_edit_jsfuncs(&self) -> T::DrawResult {
        let cont = self.controller.borrow();
        let jsfuncs = cont.list_jsfuncs();
        self.ui_toolkit.draw_all(jsfuncs.iter().map(|f| self.render_edit_jsfunc(f)).collect())
    }

    fn render_edit_jsfunc(&self, jsfunc: &jsstuff::JSFunc) -> T::DrawResult {
        self.ui_toolkit.draw_window(&format!("Edit JSFunc: {}", jsfunc.id), &|| {
            let cont1 = Rc::clone(&self.controller);
            let jsfunc1 = jsfunc.clone();
            let cont3 = Rc::clone(&self.controller);
            let jsfunc3 = jsfunc.clone();

            self.ui_toolkit.draw_all(vec![
                self.ui_toolkit.draw_text_input_with_label(
                    "Function name",
                    jsfunc.name(),
                    move |newvalue| {
                        let mut jsfunc1 = jsfunc1.clone();
                        jsfunc1.name = newvalue.to_string();
                        cont1.borrow_mut().load_function(jsfunc1);
                    },
                    || {},
                ),
                self.ui_toolkit.draw_multiline_text_input_with_label(
                    "Code",
                    &jsfunc.eval,
                    move |newvalue| {
                        let mut jsfunc3 = jsfunc3.clone();
                        jsfunc3.eval = newvalue.to_string();
                        cont3.borrow_mut().execution_environment.add_function(Box::new(jsfunc3));
                    },
                ),
                self.render_return_type_selector(jsfunc),
                self.ui_toolkit.draw_separator(),
                self.render_test_section(jsfunc.clone()),
                self.ui_toolkit.draw_separator(),
                self.render_general_function_menu(jsfunc),
            ])
        },
        None::<fn(Keypress)>)
    }

    fn get_struct(&self, struct_id: lang::ID) -> Option<structs::Struct> {
        let typespec = self.controller.borrow().get_typespec(struct_id).cloned()?;
        typespec.downcast::<structs::Struct>().map(|bawx| *bawx).ok()
    }

    fn render_edit_structs(&self) -> T::DrawResult {
        let structs = self.controller.borrow().list_structs().cloned().collect_vec();
        self.ui_toolkit.draw_all(structs.iter().map(|s| self.render_edit_struct(s)).collect())
    }

    fn render_edit_struct(&self, strukt: &structs::Struct) -> T::DrawResult {
        self.ui_toolkit.draw_window(
            &format!("Edit Struct: {}", strukt.id),
            &|| {
                let cont1 = Rc::clone(&self.controller);
                let strukt1 = strukt.clone();
                let cont2 = Rc::clone(&self.controller);
                let strukt2 = strukt.clone();

                self.ui_toolkit.draw_all(vec![
                    self.ui_toolkit.draw_text_input_with_label(
                        "Structure name",
                        &strukt.name,
                        move |newvalue| {
                            let mut strukt = strukt1.clone();
                            strukt.name = newvalue.to_string();
                            cont1.borrow_mut().load_typespec(strukt);
                        },
                        &|| {}
                    ),
                    self.ui_toolkit.draw_text_input_with_label(
                        "Symbol",
                        &strukt.symbol,
                        move |newvalue| {
                            let mut strukt = strukt2.clone();
                            strukt.symbol = newvalue.to_string();
                            cont2.borrow_mut().load_typespec(strukt);
                        },
                        &|| {},
                    ),
                    self.render_struct_fields_selector(strukt),
                    self.render_general_struct_menu(strukt),
                ])
            },
            None::<fn(Keypress)>,
        )
    }

    // TODO: this is super dupe of render_arguments_selector, whatever for now but we'll
    // clean this up
    // TODO: fix this it looks like shit
    fn render_struct_fields_selector(&self, strukt: &structs::Struct) -> T::DrawResult {
        let fields = &strukt.fields;

        let mut to_draw = vec![
            self.ui_toolkit.draw_text_with_label(&format!("Has {} field(s)", fields.len()),
                                                 "Fields"),
        ];

        for (current_field_index, field) in fields.iter().enumerate() {
            let strukt1 = strukt.clone();
            let cont1 = Rc::clone(&self.controller);
            to_draw.push(self.ui_toolkit.draw_text_input_with_label(
                "Name",
                &field.name,
                move |newvalue| {
                    let mut newstrukt = strukt1.clone();
                    let mut newfield = &mut newstrukt.fields[current_field_index];
                    newfield.name = newvalue.to_string();
                    cont1.borrow_mut().load_typespec(newstrukt)
                },
                &||{}));

            let strukt1 = strukt.clone();
            let cont1 = Rc::clone(&self.controller);
            to_draw.push(self.render_type_change_combo(
                "Type",
                &field.field_type,
                move |newtype| {
                    let mut newstrukt = strukt1.clone();
                    let mut newfield = &mut newstrukt.fields[current_field_index];
                    newfield.field_type = newtype;
                    cont1.borrow_mut().load_typespec(newstrukt)
                }
            ));

           let strukt1 = strukt.clone();
           let cont1 = Rc::clone(&self.controller);
           to_draw.push(self.ui_toolkit.draw_button(
               "Delete",
               RED_COLOR,
               move || {
                   let mut newstrukt = strukt1.clone();
                   newstrukt.fields.remove(current_field_index);
                   cont1.borrow_mut().load_typespec(newstrukt)
               }
           ));
        }

        let strukt1 = strukt.clone();
        let cont1 = Rc::clone(&self.controller);
        to_draw.push(self.ui_toolkit.draw_button("Add another field", GREY_COLOR, move || {
            let mut newstrukt = strukt1.clone();
            newstrukt.fields.push(structs::StructField::new(
                format!("field{}", newstrukt.fields.len()),
                lang::Type::from_spec(&*lang::NULL_TYPESPEC),
            ));
            cont1.borrow_mut().load_typespec(newstrukt);
        }));

        self.ui_toolkit.draw_all(to_draw)
    }

    // TODO: a way to delete the struct :)
    fn render_general_struct_menu(&self, _strukt: &structs::Struct) -> T::DrawResult {
        self.ui_toolkit.draw_all(vec![
        ])
    }

    fn render_general_function_menu<F: lang::Function>(&self, func: &F) -> T::DrawResult {
        let cont1 = Rc::clone(&self.controller);
        let func_id = func.id();
        self.ui_toolkit.draw_all(vec![
            self.ui_toolkit.draw_button("Delete", RED_COLOR, move || {
                cont1.borrow_mut().remove_function(func_id);
            })
        ])
    }

    fn render_arguments_selector<F: external_func::ModifyableFunc>(&self, func: F) -> T::DrawResult {
        let args = func.takes_args();

        let mut to_draw = vec![
            self.ui_toolkit.draw_text_with_label(&format!("Takes {} argument(s)", args.len()),
                                                 "Arguments"),
        ];

        for (current_arg_index, arg) in args.iter().enumerate() {
            let func1 = func.clone();
            let args1 = args.clone();
            let cont1 = Rc::clone(&self.controller);
            to_draw.push(self.ui_toolkit.draw_text_input_with_label(
                "Name",
                &arg.short_name,
                move |newvalue| {
                    let mut newfunc = func1.clone();
                    let mut newargs = args1.clone();
                    let mut newarg = &mut newargs[current_arg_index];
                    newarg.short_name = newvalue.to_string();
                    newfunc.set_args(newargs);
                    cont1.borrow_mut().load_function(newfunc)
                },
                &||{}));

            let func1 = func.clone();
            let args1 = args.clone();
            let cont1 = Rc::clone(&self.controller);
            to_draw.push(self.render_type_change_combo(
                "Type",
                &arg.arg_type,
                move |newtype| {
                    let mut newfunc = func1.clone();
                    let mut newargs = args1.clone();
                    let mut newarg = &mut newargs[current_arg_index];
                    newarg.arg_type = newtype;
                    newfunc.set_args(newargs);
                    cont1.borrow_mut().load_function(newfunc)
                }
            ));

            let func1 = func.clone();
            let args1 = args.clone();
            let cont1 = Rc::clone(&self.controller);
            to_draw.push(self.ui_toolkit.draw_button(
                "Delete",
                RED_COLOR,
                move || {
                    let mut newfunc = func1.clone();
                    let mut newargs = args1.clone();
                    newargs.remove(current_arg_index);
                    newfunc.set_args(newargs);
                    cont1.borrow_mut().load_function(newfunc)
                }
            ));
        }

        let func1 = func.clone();
        let args1 = args.clone();
        let cont1 = Rc::clone(&self.controller);
        to_draw.push(self.ui_toolkit.draw_button("Add another argument", GREY_COLOR, move || {
            let mut args = args1.clone();
            let mut func = func1.clone();
            args.push(lang::ArgumentDefinition::new(
                lang::Type::from_spec(&*lang::NULL_TYPESPEC),
                format!("arg{}", args.len()),
            ));
            func.set_args(args);
            cont1.borrow_mut().load_function(func);
        }));

        self.ui_toolkit.draw_all(to_draw)
    }

    fn render_typespec_selector_with_label<F>(&self, label: &str, selected_ts_id: ID,
                                              nesting_level: Option<&[usize]>, onchange: F) -> T::DrawResult
        where F: Fn(&Box<lang::TypeSpec>) + 'static
    {
        // TODO: pretty sure we can get rid of the clone and let the borrow live until the end
        // but i don't want to mess around with it right now
        let selected_ts = self.controller.borrow().get_typespec(selected_ts_id).unwrap().clone();
        let typespecs = self.controller.borrow().typespecs().into_iter()
            .map(|ts| ts.clone()).collect_vec();
        self.ui_toolkit.draw_combo_box_with_label(
            label,
            |ts| ts.matches(selected_ts.id()),
            |ts| format_typespec_select(ts, nesting_level),
            &typespecs.iter().collect_vec(),
            move |newts| { onchange(newts) }
        )
    }

    fn render_type_change_combo<F>(&self, label: &str, typ: &lang::Type, onchange: F) -> T::DrawResult
        where F: Fn(lang::Type) + 'static {
        let type1 = typ.clone();
        let onchange = Rc::new(onchange);
        let onchange2 = Rc::clone(&onchange);
        self.ui_toolkit.draw_all(vec![
            self.render_typespec_selector_with_label(
                label,
                typ.typespec_id,
                None,
                move |new_ts| {
                    let mut newtype = type1.clone();
                    edit_types::set_typespec(&mut newtype, new_ts, &[]);
                    onchange(newtype)
                }
            ),
            self.render_type_params_change_combo(typ, onchange2, &[])
        ])
    }

    fn render_type_params_change_combo<F>(&self, root_type: &lang::Type, onchange: Rc<F>,
                                          nesting_level: &[usize]) -> T::DrawResult
        where F: Fn(lang::Type) + 'static
    {
        let mut type_to_change = root_type.clone();
        let mut type_to_change = &mut type_to_change;
        for param_index in nesting_level.into_iter() {
            type_to_change = &mut type_to_change.params[*param_index]
        }

        let mut drawn = vec![];
        for (i, param) in type_to_change.params.iter().enumerate() {
            let mut new_nesting_level = nesting_level.to_owned();
            new_nesting_level.push(i);

            let onchange = Rc::clone(&onchange);
            let onchange2 = Rc::clone(&onchange);
            let nnl = new_nesting_level.clone();
            let root_type1 = root_type.clone();
            drawn.push(
                self.render_typespec_selector_with_label(
                    "",
                    param.typespec_id,
                    Some(nesting_level),
                    move |new_ts| {
                        let mut newtype = root_type1.clone();
                        edit_types::set_typespec(&mut newtype, new_ts, &nnl);
                        onchange(newtype)
                    }
                ),
            );
            drawn.push(self.render_type_params_change_combo(root_type, onchange2, &new_nesting_level));
        }
        self.ui_toolkit.draw_all(drawn)
    }

    fn render_return_type_selector<F: external_func::ModifyableFunc>(&self, func: &F) -> T::DrawResult {
        // TODO: why doesn't this return a reference???
        let return_type = func.returns();

        let cont = Rc::clone(&self.controller);
        let pyfunc2 = func.clone();

        self.ui_toolkit.draw_all(vec![
            self.render_type_change_combo(
                "Return type",
                &return_type,
                move |newtype| {
                    let mut newfunc = pyfunc2.clone();
                    newfunc.set_return_type(newtype);
                    cont.borrow_mut().load_function(newfunc)
                }
            ),
        ])
    }

    fn render_test_section<F: lang::Function>(&self, func: F) -> T::DrawResult {
        let test_result = self.controller.borrow_mut().get_test_result(&func);
        let cont = Rc::clone(&self.controller);
        self.ui_toolkit.draw_all(vec![
            self.ui_toolkit.draw_text(&format!("Test result: {}", test_result)),
            self.ui_toolkit.draw_button("Run", GREY_COLOR, move || {
                cont.borrow_mut().run_test(&func)
            })
        ])
    }

    fn render_status_bar(&self) -> T::DrawResult {
        self.ui_toolkit.draw_statusbar(&|| {
            if let Some(node) = self.controller.borrow().get_selected_node() {
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
        None::<fn(Keypress)>)
    }

    fn render_error_window(&self) -> T::DrawResult {
        let controller = self.controller.clone();
        self.ui_toolkit.draw_window("Errors", &|| {
            self.ui_toolkit.draw_text_box(controller.borrow().read_error_console())
        },
        None::<fn(Keypress)>)
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
                    Some(move |keypress| {
                        let mut controller = cont.borrow_mut();
                        controller.handle_keypress_in_code_window(keypress)
                    }))
            }
        }
    }

    fn render_code(&self, code_node: &CodeNode) -> T::DrawResult {
        if self.is_editing(code_node.id()) {
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
                CodeNode::FunctionDefinition(_function_definition) => {
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
                CodeNode::NullLiteral => {
                    self.ui_toolkit.draw_text(&format!(" {} ", lang::NULL_TYPESPEC.symbol))
                },
                CodeNode::StructLiteral(struct_literal) => {
                    self.render_struct_literal(&struct_literal)
                },
                CodeNode::StructLiteralField(_field) => {
                    self.ui_toolkit.draw_all(vec![])
                    // we would, except render_struct_literal_field isn't called from here...
                    // ......... lols
                    //self.render_struct_literal_field(&field)
                },
            }
        };

        if self.is_selected(code_node.id()) {
            self.draw_selected(&draw)
        } else {
            self.draw_code_node_and_insertion_point_if_before_or_after(code_node, &draw)
        }
    }

    fn draw_selected(&self, draw: &Fn() -> T::DrawResult) -> T::DrawResult {
        self.ui_toolkit.draw_box_around(SELECTION_COLOR, draw)
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
                let new_code_node = menu.selected_option_code(&self.controller.borrow().code_genie().unwrap());

                self.ui_toolkit.draw_text_input(
                    menu.search_str(),
                    move |input|{
                        controller_1.borrow_mut().insert_code_menu.as_mut()
                            .map(|m| {m.set_search_str(input)});
                    },
                    move ||{
                        let mut controller = controller_2.borrow_mut();
                        if let Some(ref new_code_node) = new_code_node {
                            controller.hide_insert_code_menu();
                            controller.insert_code(new_code_node.clone(), insertion_point);
                        } else {
                            controller.undo();
                            controller.hide_insert_code_menu();
                        }
                    })
            }),
            self.render_insertion_options(&menu)
        ])
    }

    fn render_insertion_options(&self, menu: &InsertCodeMenu) -> <T as UiToolkit>::DrawResult {
        let options = menu.list_options(&self.controller.borrow().code_genie().unwrap());
        let render_insertion_options : Vec<Box<Fn() -> T::DrawResult>> = options.iter()
            .map(|option| {
                let c : Box<Fn() -> T::DrawResult> = Box::new(move || {
                    self.render_insertion_option(option, menu.insertion_point)
                });
                c
            })
            .collect();
        self.ui_toolkit.draw_all_on_same_line(
            &render_insertion_options.iter()
                .map(|c| c.as_ref()).collect_vec()
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
            self.draw_selected(&draw)
        } else {
            draw()
        }
    }

    fn render_assignment(&self, assignment: &Assignment) -> T::DrawResult {
        self.ui_toolkit.draw_all_on_same_line(&[
            &|| {
                self.render_inline_editable_button(
                    &assignment.name,
                    PURPLE_COLOR,
                    assignment.id
                )
            },
            &|| { self.ui_toolkit.draw_text(" = ") },
            &|| { self.render_code(assignment.expression.as_ref()) }
        ])
    }

    fn render_variable_reference(&self, variable_reference: &VariableReference) -> T::DrawResult {
        let controller = self.controller.borrow();
        let loaded_code = controller.loaded_code.as_ref().unwrap();
        let assignment = loaded_code.find_node(variable_reference.assignment_id);
        if let Some(CodeNode::Assignment(assignment)) = assignment {
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
        // XXX: we've gotta have this conditional because of a quirk with the way the imgui
        // toolkit works. if render_function_call_arguments doesn't actually draw anything, it
        // will cause the next drawn thing to appear on the same line. weird i know, maybe we can
        // one day fix this jumbledness
        if function_call.args.is_empty() {
            self.render_code(&function_call.function_reference)
        } else {
            self.ui_toolkit.draw_all_on_same_line(&[
                &|| { self.render_code(&function_call.function_reference) },
                &|| {
                    self.render_function_call_arguments(
                        function_call.function_reference().function_id,
                        function_call.args())
                },
            ])
        }
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

        if let Some(function) = self.controller.borrow_mut().find_function(function_id) {
            color = BLUE_COLOR;
            function_name = function.name().to_string();
        }
        self.ui_toolkit.draw_button(&function_name, color, &|| {})
    }

    fn render_function_call_arguments(&self, function_id: ID, args: Vec<&lang::Argument>) -> T::DrawResult {
        let top_border_thickness = 1;
        let right_border_thickness = 1;
        let bottom_border_thickness = 1;

        let draw = || {
            let function = self.controller.borrow().find_function(function_id)
                .map(|func| func.clone());
            let args = args.clone();
            match function {
                Some(function) => {
                    self.render_args_for_found_function(&*function, args)
                },
                None => {
                    self.render_args_for_missing_function(args)
                }
            }
        };

        let nesting_level = self.arg_nesting_level.replace_with(|i| *i + 1);
        let top_border_thickness = top_border_thickness + nesting_level + 1;
        let drawn = self.ui_toolkit.draw_top_border_inside(BLACK_COLOR, top_border_thickness as u8, &|| {
            self.ui_toolkit.draw_right_border_inside(BLACK_COLOR, right_border_thickness, &|| {
                self.ui_toolkit.draw_bottom_border_inside(BLACK_COLOR, bottom_border_thickness, &draw)
            })
        });
        self.arg_nesting_level.replace_with(|i| *i - 1);
        drawn
    }

    fn get_symbol_for_type(&self, t: &lang::Type) -> String {
        self.controller.borrow().get_typespec(t.typespec_id).unwrap().symbol().to_string()
    }

    fn render_function_call_argument(&self, argument: &lang::Argument) -> T::DrawResult {
        let type_symbol = {
            let controller = self.controller.borrow();
            let genie = controller.code_genie().unwrap();
            match genie.get_type_for_arg(argument.argument_definition_id) {
                Some(arg_type) => self.get_symbol_for_type(&arg_type),
                None => "\u{f059}".to_string(),
            }
        };
        self.ui_toolkit.draw_all_on_same_line(&[
            &|| {
                self.render_inline_editable_button(&type_symbol, BLACK_COLOR, argument.id)
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
            let c : Box<Fn() -> T::DrawResult> =
                if let Some(provided_arg) = provided_arg_by_definition_id.get(&expected_arg.id) {
                    Box::new(move || self.render_code(&CodeNode::Argument(provided_arg.clone())))
                } else {
                    Box::new(move || self.render_missing_function_argument(expected_arg))
                };
            c
        }).collect_vec();

        self.ui_toolkit.draw_all_on_same_line(&draw_results.iter().map(|c| c.as_ref()).collect_vec())
    }

    fn render_missing_function_argument(&self, _arg: &lang::ArgumentDefinition) -> T::DrawResult {
        self.ui_toolkit.draw_button(
            "this shouldn't have happened, you've got a missing function arg somehow",
            RED_COLOR,
            &|| {})
    }

    fn render_args_for_missing_function(&self, _args: Vec<&lang::Argument>) -> T::DrawResult {
        // TODO: implement this
        self.ui_toolkit.draw_all(vec![])
    }

    fn render_struct_literal(&self, struct_literal: &lang::StructLiteral) -> T::DrawResult {
        // XXX: we've gotta have this conditional because of a quirk with the way the imgui
        // toolkit works. if render_function_call_arguments doesn't actually draw anything, it
        // will cause the next drawn thing to appear on the same line. weird i know, maybe we can
        // one day fix this jumbledness
        let strukt = self.get_struct(struct_literal.struct_id).unwrap();

        if struct_literal.fields.is_empty() {
            self.render_struct_identifier(&strukt, struct_literal)
        } else {
            self.ui_toolkit.draw_all_on_same_line(&[
                &|| { self.render_struct_identifier(&strukt, struct_literal) },
                &|| {
                    self.render_struct_literal_fields(&strukt,
                                                      &struct_literal.fields())
                },
            ])
        }
    }

    fn render_struct_identifier(&self, strukt: &structs::Struct,
                                _struct_literal: &lang::StructLiteral) -> T::DrawResult {
        // TODO: handle when the typespec ain't available
        self.ui_toolkit.draw_button(&strukt.name, SALMON_COLOR, &|| {})
    }

    fn render_struct_literal_fields(&self, strukt: &structs::Struct,
                                    fields: &[&lang::StructLiteralField]) -> T::DrawResult {
        // TODO: should this map just go inside the struct????
        let struct_field_by_id = strukt.field_by_id();

        let mut to_draw : Vec<Box<Fn() -> T::DrawResult>> = vec![];
        for literal_field in fields.iter() {
            // this is where the bug is
            let strukt_field = struct_field_by_id.get(&literal_field.struct_field_id).unwrap();
            to_draw.push(Box::new(move || {
                self.render_struct_literal_field(strukt_field, literal_field)
            }));
        }
        self.ui_toolkit.draw_all_on_same_line(&to_draw.iter().map(|td| td.as_ref()).collect_vec())
    }

    fn render_struct_literal_field(&self, field: &structs::StructField,
                                   literal: &lang::StructLiteralField) -> T::DrawResult {
        let field_text = format!("{} {}", self.get_symbol_for_type(&field.field_type),
                                        field.name);
        self.ui_toolkit.draw_all_on_same_line(&[
            &|| {
                if self.is_editing(literal.id) {
                    self.render_insert_code_node()
                } else {
                    self.render_inline_editable_button(&field_text, BLACK_COLOR, literal.id)
                }
            },
            &|| { self.render_code(&literal.expr) }
        ])
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

    fn render_string_literal(&self, string_literal: &StringLiteral) -> T::DrawResult {
        self.render_inline_editable_button(
            &format!("\u{F10D} {} \u{F10E}", string_literal.value),
            CLEAR_COLOR,
            string_literal.id)
    }

    fn render_run_button(&self, code_node: &CodeNode) -> T::DrawResult {
        let controller = self.controller.clone();
        let code_node = code_node.clone();
        self.ui_toolkit.draw_button("Run", GREY_COLOR, move ||{
            let mut controller = controller.borrow_mut();
            controller.run(&code_node);
        })
    }

    fn render_inline_editable_button(&self, label: &str, color: Color, code_node_id: lang::ID) -> T::DrawResult {
        let controller = self.controller.clone();
        self.ui_toolkit.draw_button(label, color, move || {
            let mut controller = controller.borrow_mut();
            controller.mark_as_editing(code_node_id);
        })
    }

    fn is_selected(&self, code_node_id: ID) -> bool {
        Some(code_node_id) == *self.controller.borrow().get_selected_node_id()
    }

    fn is_editing(&self, code_node_id: ID) -> bool {
        self.is_selected(code_node_id) && self.controller.borrow().editing
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
            CodeNode::Argument(_) | CodeNode::StructLiteralField(_) => {
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
                let mut controller = controller2.borrow_mut();
                controller.editing = false;
            },
            // TODO: i think we need another callback for what happens when you CANCEL
        )
    }
}

struct DeletionResult {
    new_root: CodeNode,
    new_cursor_position: Option<ID>,
}

impl DeletionResult {
    fn new(new_root: CodeNode, new_cursor_position: Option<ID>) -> Self {
        Self { new_root, new_cursor_position }
    }
}

struct MutationMaster {
    history: RefCell<undo::UndoHistory>,
}

impl MutationMaster {
    fn new() -> Self {
        MutationMaster { history: RefCell::new(undo::UndoHistory::new()) }
    }

    fn insert_code(&self, node_to_insert: &CodeNode, insertion_point: InsertionPoint,
                   genie: &CodeGenie) -> CodeNode {
        let parent = genie.find_parent(insertion_point.node_id());
        if parent.is_none() {
            panic!("unable to insert new code, couldn't find parent to insert into")

        }
        let parent = parent.unwrap();
        let node_to_insert = node_to_insert.clone();
        match insertion_point {
            InsertionPoint::Before(_) | InsertionPoint::After(_) => {
                self.insert_new_expression_in_block(
                    node_to_insert, insertion_point, parent.clone(), genie)

            }
            InsertionPoint::Argument(argument_id) => {
                self.insert_expression_into_argument(node_to_insert, argument_id, genie)
            },
            InsertionPoint::StructLiteralField(struct_literal_field_id) => {
                self.insert_expression_into_struct_literal_field(node_to_insert, struct_literal_field_id, genie)
            }
        }
    }

    fn insert_expression_into_argument(&self, code_node: CodeNode, argument_id: ID,
                                       genie: &CodeGenie) -> CodeNode {
        let mut argument = genie.find_node(argument_id).unwrap().into_argument().clone();
        argument.expr = Box::new(code_node);
        let mut root = genie.root().clone();
        root.replace(&CodeNode::Argument(argument));
        root
    }

    fn insert_expression_into_struct_literal_field(&self, code_node: CodeNode,
                                                   struct_literal_field_id: ID,
                                                   genie: &CodeGenie) -> CodeNode {
        let mut struct_literal_field = genie.find_node(struct_literal_field_id).unwrap()
            .into_struct_literal_field().unwrap().clone();
        struct_literal_field.expr = Box::new(code_node);
        let mut root = genie.root().clone();
        root.replace(&CodeNode::StructLiteralField(struct_literal_field));
        root
    }

    fn insert_new_expression_in_block(&self, code_node: CodeNode, insertion_point: InsertionPoint,
                                      parent: CodeNode, genie: &CodeGenie) -> CodeNode {
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
                root
            },
            _ => panic!("should be inserting into type parent, got {:?} instead", parent)
        }
    }

    pub fn delete_code(&self, node_to_delete: &CodeNode, genie: &CodeGenie,
                       cursor_position: Option<ID>) -> DeletionResult {
        let parent = genie.find_parent(node_to_delete.id());
        if parent.is_none() {
            panic!("idk when this happens, let's take care of this if / when it does")
        }
        let parent = parent.unwrap();
        match parent {
            CodeNode::Block(block) => {
                let mut new_block = block.clone();
                new_block.expressions.retain(|exp| exp.id() != node_to_delete.id());

                let deleted_expression_position_in_block = genie.find_position_in_block(
                    &block, node_to_delete.id()).unwrap();
                let mut new_cursor_position = new_block.expressions
                    .get(deleted_expression_position_in_block)
                    .map(|code_node| code_node.id());
                if new_cursor_position.is_none() {
                    new_cursor_position = new_block.expressions
                    .get(deleted_expression_position_in_block - 1)
                    .map(|code_node| code_node.id());
                }

                let mut new_root = genie.root().clone();
                new_root.replace(&CodeNode::Block(new_block));

//                self.log_new_mutation(&new_root, new_cursor_position);
                DeletionResult::new(new_root, new_cursor_position)
            }
            _ => {
                DeletionResult::new(genie.root().clone(), cursor_position)
            }
        }
    }

    fn log_new_mutation(&self, new_root: &CodeNode, cursor_position: Option<ID>) {
        self.history.borrow_mut().record_previous_state(new_root, cursor_position);
    }

    pub fn undo(&self, current_root: &CodeNode,
                cursor_position: Option<ID>) -> Option<undo::UndoHistoryCell> {
        self.history.borrow_mut().undo(current_root, cursor_position)
    }

    pub fn redo(&self, current_root: &CodeNode,
                cursor_position: Option<ID>) -> Option<undo::UndoHistoryCell> {
        self.history.borrow_mut().redo(current_root, cursor_position)
    }
}

// return value: (CodeNode ID, editing: true/false)
fn post_insertion_cursor(code_node: &CodeNode, code_genie: &CodeGenie) -> (ID, bool) {
    if let CodeNode::FunctionCall(function_call) = code_node {
        // if we just inserted a function call, then go to the first arg if there is one
        if function_call.args.len() > 0 {
            return (function_call.args.get(0).unwrap().id(), true)
        } else {
            return (function_call.id, false)
        }
    }

    if let CodeNode::StructLiteral(struct_literal) = code_node {
        // if we just inserted a function call, then go to the first arg if there is one
        if struct_literal.fields.len() > 0 {
            return (struct_literal.fields.get(0).unwrap().id(), true)
        } else {
            return (struct_literal.id, false)
        }
    }

    let parent = code_genie.find_parent(code_node.id());
    if let Some(CodeNode::Argument(argument)) = parent {
        // if we just finished inserting into a function call argument, and the next argument is
        // a placeholder, then let's insert into that arg!!!!
        if let Some(CodeNode::FunctionCall(function_call)) = code_genie.find_parent(argument.id) {
            let just_inserted_argument_position = function_call.args.iter()
                .position(|arg| arg.id() == argument.id).unwrap();
            let maybe_next_arg = function_call.args.get(just_inserted_argument_position + 1);
            if let Some(CodeNode::Argument(lang::Argument{ expr: box CodeNode::Placeholder(_), .. })) = maybe_next_arg {
                return (maybe_next_arg.unwrap().id(), true)
            }
        }
    } else if let Some(CodeNode::StructLiteralField(struct_literal_field)) = parent {
        // if we just finished inserting into a function call argument, and the next argument is
        // a placeholder, then let's insert into that arg!!!!
        if let Some(CodeNode::StructLiteral(struct_literal)) = code_genie.find_parent(struct_literal_field.id) {
            let just_inserted_argument_position = struct_literal.fields.iter()
                .position(|field| field.id() == struct_literal_field.id).unwrap();
            let maybe_next_field = struct_literal.fields.get(just_inserted_argument_position + 1);
            if let Some(CodeNode::StructLiteralField(lang::StructLiteralField{ expr: box CodeNode::Placeholder(_), .. })) = maybe_next_field {
                return (maybe_next_field.unwrap().id(), true)
            }
        }
    }

    // nothing that we can think of to do next, just chill at the insertion point
    (code_node.id(), false)
}

fn format_typespec_select(ts: &Box<lang::TypeSpec>, nesting_level: Option<&[usize]>) -> String {
    let indent = match nesting_level {
        Some(nesting_level) => {
            iter::repeat("\t").take(nesting_level.len() + 1).join("")
        },
        None => "".to_owned(),
    };
    format!("{}{} {}", indent, ts.symbol(), ts.readable_name())
}
