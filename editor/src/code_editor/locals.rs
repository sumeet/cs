use crate::code_editor::{CodeGenie, InsertionPoint};
use crate::insert_code_menu;
use cs::{lang, EnvGenie};

use cs::lang::arg_typ_for_anon_func;
use serde_derive::{Deserialize, Serialize};

// just need this for debugging, tho maybe i'll keep it around, it's probably good to have
#[derive(Serialize, Deserialize, Debug)]
enum VariableAntecedent {
    Assignment {
        assignment_id: lang::ID,
    },
    AnonFuncArgument {
        anonymous_function_id: lang::ID,
    },
    Argument,
    MatchVariant {
        match_statement_id: lang::ID,
        variant_id: lang::ID,
    },
}

pub fn resolve_generics(variable: &Variable,
                        code_genie: &CodeGenie,
                        env_genie: &EnvGenie)
                        -> lang::Type {
    match variable.variable_type {
        VariableAntecedent::Assignment { assignment_id } => {
            let code_node = code_genie.find_node(assignment_id).unwrap();
            code_genie.try_to_resolve_all_generics(code_node, variable.typ.clone(), env_genie)
        }
        VariableAntecedent::AnonFuncArgument { anonymous_function_id, } => {
            let anon_func = code_genie.find_node(anonymous_function_id).unwrap();
            let full_unresolved_typ = code_genie.guess_type_without_resolving_generics(anon_func,
                                                                                       env_genie)
                                                .unwrap();
            let resolved_anon_func_typ =
                code_genie.try_to_resolve_all_generics(anon_func, full_unresolved_typ, env_genie);
            arg_typ_for_anon_func(resolved_anon_func_typ)
        }
        VariableAntecedent::Argument => {
            // unhandled so far
            variable.typ.clone()
        }

        VariableAntecedent::MatchVariant { .. } => {
            // also unhandled
            variable.typ.clone()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Variable {
    variable_type: VariableAntecedent,
    pub locals_id: lang::ID,
    pub(crate) typ: lang::Type,
    pub(crate) name: String,
}

fn find_anon_func_args_for<'a>(search_position: SearchPosition,
                               code_genie: &'a CodeGenie,
                               env_genie: &'a EnvGenie)
                               -> impl Iterator<Item = Variable> + 'a {
    println!("findind anon func parents");
    let p = code_genie.find_anon_func_parents(search_position.before_code_id)
                      .map(move |anon_func| {
                          let arg = &anon_func.as_anon_func().unwrap().takes_arg;
                          println!("guessing type without resolving for {:?}", anon_func);
                          let anon_func_typ =
                              code_genie.guess_type_without_resolving_generics(anon_func,
                                                                               env_genie)
                                        .unwrap();
                          // println!("arg name: {:?}", arg.short_name);
                          // println!("guessed typ for anon_func: {:?}", anon_func_typ);
                          // println!("variable typ: {:?}", anon_func_typ.params[0]);
                          Variable { variable_type:
                                 VariableAntecedent::AnonFuncArgument { anonymous_function_id:
                                                                            anon_func.id() },
                             locals_id: arg.id,
                             typ: arg_typ_for_anon_func(anon_func_typ),
                             name: arg.short_name.clone() }
                      })
                      .collect::<Vec<_>>();
    println!("done finding anon func parents");
    p.into_iter()
}

#[derive(Copy, Clone)]
pub struct SearchPosition {
    pub before_code_id: lang::ID,
    pub is_search_inclusive: bool,
}

impl SearchPosition {
    pub fn not_inclusive(before_id: lang::ID) -> Self {
        Self { before_code_id: before_id,
               is_search_inclusive: false }
    }
}

impl From<InsertionPoint> for SearchPosition {
    fn from(ip: InsertionPoint) -> Self {
        let (insertion_id, is_search_inclusive) = insert_code_menu::assignment_search_position(ip);
        SearchPosition { before_code_id: insertion_id,
                         is_search_inclusive }
    }
}

// TODO: this should probably go near the code genie
pub fn find_all_locals_preceding_with_resolving_generics<'a>(
    search_position: SearchPosition,
    code_genie: &'a CodeGenie,
    env_genie: &'a EnvGenie)
    -> impl Iterator<Item = Variable> + 'a {
    find_all_locals_preceding_without_resolving_generics(search_position, code_genie, env_genie).map(move |mut var| {
        println!("found a generic: {:?}", var);
        var.typ = resolve_generics(&var, code_genie, env_genie);
        println!("done with generic: {:?}", var);
        var
    })
}

pub fn find_all_locals_preceding_without_resolving_generics<'a>(
    search_position: SearchPosition,
    code_genie: &'a CodeGenie,
    env_genie: &'a EnvGenie)
    -> impl Iterator<Item = Variable> + 'a {
    find_assignments_and_function_args_preceding(search_position, code_genie, env_genie)
        .chain(find_enum_variants_preceding(search_position, code_genie, env_genie))
        .chain(find_anon_func_args_for(search_position, code_genie, env_genie))
}

pub fn find_assignments_preceding<'a>(search_position: SearchPosition,
                                      code_genie: &'a CodeGenie,
                                      env_genie: &'a EnvGenie)
                                      -> impl Iterator<Item = Variable> + 'a {
    code_genie.find_assignments_that_come_before_code(search_position.before_code_id,
                                                      search_position.is_search_inclusive)
              .into_iter()
              .map(move |assignment| {
                  let assignment_clone: lang::Assignment = (*assignment).clone();
                  let guessed_type =
                      code_genie.guess_type_without_resolving_generics(&lang::CodeNode::Assignment(assignment_clone),
                                            env_genie);
                  Variable { locals_id: assignment.id,
                             variable_type: VariableAntecedent::Assignment { assignment_id: assignment.id },
                             typ: guessed_type.unwrap(),
                             name: assignment.name.clone() }
              })
}

pub fn find_assignments_and_function_args_preceding<'a>(search_position: SearchPosition,
                                                        code_genie: &'a CodeGenie,
                                                        env_genie: &'a EnvGenie)
                                                        -> impl Iterator<Item = Variable> + 'a {
    find_assignments_preceding(search_position, code_genie, env_genie)
              .chain(env_genie.code_takes_args(code_genie.root().id())
                              .map(|arg| Variable { locals_id: arg.id,
                                                    variable_type: VariableAntecedent::Argument,
                                                    typ: arg.arg_type,
                                                    name: arg.short_name }))
}

fn find_enum_variants_preceding<'a>(search_position: SearchPosition,
                                    code_genie: &'a CodeGenie,
                                    env_genie: &'a EnvGenie)
                                    -> impl Iterator<Item = Variable> + 'a {
    code_genie.find_enum_variants_preceding_iter(search_position.before_code_id, env_genie)
              .map(|match_variant| {
                  Variable { locals_id: match_variant.assignment_id(),
                             variable_type:
                                 VariableAntecedent::MatchVariant { match_statement_id:
                                                                        match_variant.match_id,
                                                                    variant_id:
                                                                        match_variant.enum_variant
                                                                                     .id },
                             typ: match_variant.typ,
                             name: match_variant.enum_variant.name }
              })
}
