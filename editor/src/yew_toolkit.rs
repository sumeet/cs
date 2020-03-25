mod focus;
mod run_after_render;

use super::app::App as CSApp;
use super::async_executor::AsyncExecutor;
use super::editor::{Key as AppKey, Keypress};
use super::ui_toolkit::UiToolkit;
use crate::code_editor_renderer::BLACK_COLOR;
use crate::colorscheme;
use crate::ui_toolkit::{ChildRegionHeight, Color, DrawFnRef, SelectableItem};

use std::cell::RefCell;
use std::rc::Rc;

use itertools::Itertools;
use stdweb::js;
use stdweb::traits::IEvent;
use stdweb::traits::IKeyboardEvent;
use stdweb::unstable::TryInto;
use stdweb::web::html_element::InputElement;
use stdweb::web::{document, Element, HtmlElement, IEventTarget, IHtmlElement};
use yew::html;
use yew::prelude::*;
use yew::virtual_dom::VTag;
use yew::virtual_dom::{VList, VNode};

macro_rules! num {
    ($to_type:ident, $stdweb_value:expr) => {{
        let float: f64 = $stdweb_value.try_into().unwrap();
        float as $to_type
    }};
}

pub struct Model {
    app: Option<Rc<RefCell<CSApp>>>,
    async_executor: Option<AsyncExecutor>,
    renderer_state: Option<Rc<RefCell<RendererState>>>,
}

pub enum Msg {
    Init(Rc<RefCell<CSApp>>,
         AsyncExecutor,
         Rc<RefCell<RendererState>>),
    Redraw,
    DontRedraw,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Model { app: None,
                async_executor: None,
                renderer_state: None }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Init(app, mut async_executor, renderer_state) => {
                // flush commands initially before rendering for the first time
                app.borrow_mut().flush_commands(&mut async_executor);

                self.async_executor = Some(async_executor);
                self.app = Some(app);
                self.renderer_state = Some(renderer_state);
                true
            }
            Msg::Redraw => {
                if let (Some(app), Some(mut async_executor)) =
                    (self.app.as_ref(), self.async_executor.as_mut())
                {
                    app.borrow_mut().flush_commands(&mut async_executor);
                }
                true
            }
            Msg::DontRedraw => false,
        }
    }

    fn view(&self) -> Html<Self> {
        if let (Some(app), Some(renderer_state)) = (&self.app, &self.renderer_state) {
            let mut tk = YewToolkit::new(Rc::clone(renderer_state));
            let drawn = app.borrow_mut().draw(&mut tk);
            drawn
        } else {
            html! { <p> {"No app"} </p> }
        }
    }
}

struct YewToolkit {
    renderer_state: Rc<RefCell<RendererState>>,
}

impl UiToolkit for YewToolkit {
    type DrawResult = Html<Model>;

    // see autoscroll.js
    fn scrolled_to_y_if_not_visible(&self,
                                    _scroll_hash: String,
                                    draw_fn: &dyn Fn() -> Self::DrawResult)
                                    -> Self::DrawResult {
        html! {
            <div class="scroll-into-view",>
                {{ draw_fn() }}
            </div>
        }
    }

    fn open_file_open_dialog(callback: impl Fn(&[u8]) + 'static) {
        let callback = move |value: stdweb::Value| {
            let array_buffer: stdweb::web::ArrayBuffer = value.try_into().unwrap();
            let vu8: Vec<u8> = array_buffer.into();
            callback(&vu8);
        };
        js! { openFileDialog(@{callback}); }
    }

    fn open_file_save_dialog(filename_suggestion: &str, bytes: &[u8], mimetype: &str) {
        js! { saveFile(@{bytes}, @{filename_suggestion}, @{mimetype}); }
    }

    fn draw_color_picker_with_label(&self,
                                    label: &str,
                                    existing_value: Color,
                                    onchange: impl Fn(Color) + 'static)
                                    -> Self::DrawResult {
        let renderer_state = Rc::clone(&self.renderer_state);
        let onchange = move |value: stdweb::Value| {
            let vec: Vec<f64> = value.try_into().unwrap();
            let color: Color = [vec[0] as f32, vec[1] as f32, vec[2] as f32, vec[3] as f32];
            onchange(color);
            // TODO: for some reason, this only works if we fire this twice, probably good to figure
            // out why :)
            renderer_state.borrow().send_msg(Msg::Redraw);
            renderer_state.borrow().send_msg(Msg::Redraw);
        };
        let onchange_js = js! {
            return function(color) {
                const rgba = color.toRgb();
                @{onchange}([rgba.r / 255, rgba.g / 255, rgba.b / 255, rgba.a]);
            };
        };
        let input: Html<Model> = run_after_render::run(html! {
                                                           <input type="color", name=label />
                                                       },
                                                       move |el| {
                                                           js! {
                                                               $(@{el})
                                                                 .spectrum({change: @{&onchange_js}, move: @{&onchange_js}, showInput: true, showAlpha: true,
                                                                            preferredFormat: "hex", color: @{rgba(existing_value)}});
                                                           };
                                                       });
        html! {
            <div>
                {{ input }}
                <label>{label}</label>
            </div>
        }
    }

    fn draw_top_right_overlay(&self, draw_fn: &dyn Fn() -> Self::DrawResult) -> Self::DrawResult {
        // 35px is hardcoded to dodge the menubar
        html! {
            <div style={ format!("padding: 0.5em; position: absolute; top: 35px; right: 10px; color: white; background-color: {}",rgba(colorscheme!(window_bg_color))) }, >
                {{ draw_fn() }}
            </div>
        }
    }

    fn draw_top_left_overlay(&self, draw_fn: &dyn Fn() -> Self::DrawResult) -> Self::DrawResult {
        // 35px is hardcoded to dodge the menubar
        html! {
            <div style={ format!("padding: 0.5em; position: absolute; top: 35px; left: 10px; color: white; background-color: {}",rgba(colorscheme!(window_bg_color))) }, >
                {{ draw_fn() }}
            </div>
        }
    }

    fn draw_spinner(&self) -> Self::DrawResult {
        html! {
            <div class="spinner", >
                {" "}
            </div>
        }
    }

    fn draw_centered_popup<F: Fn(Keypress) + 'static>(&self,
                                                      draw_fn: &dyn Fn() -> Self::DrawResult,
                                                      handle_keypress: Option<F>)
                                                      -> Self::DrawResult {
        let handle_keypress_1 = Rc::new(move |keypress: Keypress| {
            if let Some(handle_keypress) = &handle_keypress {
                handle_keypress(keypress)
            }
        });
        let handle_keypress_2 = Rc::clone(&handle_keypress_1);
        let global_keydown_handler = self.global_keydown_handler();
        html! {
            <div style={ format!("background-color: {}; width: 300px; height: 200px; position: absolute; top: calc(50% - 300px); left: calc(50% - 300px); color: white; overflow: auto;", rgba(colorscheme!(window_bg_color))) },
                 tabindex=0,
                 onkeypress=|e| {
                     if let Some(keypress) = map_keypress_event(&e) {
                         handle_keypress_1(keypress);
                     }
                     e.prevent_default();
                     Msg::Redraw
                 },
                 onkeydown=|e| {
                     global_keydown_handler(&e);
                     // lol for these special keys we have to listen on keydown, but the
                     // rest we can do keypress :/
                     if e.key() == "Tab" || e.key() == "Escape" || e.key() == "Esc" ||
                         // LOL this is for ctrl+r
                         ((e.key() == "r" || e.key() == "R") && e.ctrl_key()) {
                         //console!(log, e.key());
                         if let Some(keypress) = map_keypress_event(&e) {
                             //console!(log, format!("{:?}", keypress));
                             handle_keypress_2(keypress);
                         }
                         e.prevent_default();
                         Msg::Redraw
                     } else {
                         Msg::DontRedraw
                     }
                 }, >
                {{ draw_fn() }}
            </div>
        }
    }

    fn draw_all(&self, draw_fns: &[DrawFnRef<Self>]) -> Self::DrawResult {
        html! {
            <div class="all-drawn", style="display: flex; flex-direction: column;",>
                { for draw_fns.into_iter().map(|draw_fn| html! {
                    { draw_fn() }
                })}
            </div>
        }
    }

    fn draw_separator(&self) -> Self::DrawResult {
        html! {
            <hr />
        }
    }

    fn draw_text_input_with_label<F: Fn(&str) -> () + 'static, D: Fn() + 'static>(
        &self,
        label: &str,
        existing_value: &str,
        onchange: F,
        ondone: D)
        -> Self::DrawResult {
        html! {
            // min-height: fit-content is a fix for safari. otherwise this doesn't take up any space
            // and gets stomped in flex layouts
            <div style="display: flex; min-height: fit-content;",>
                {{ self.draw_text_input(existing_value, onchange, ondone) }}
                <label>{{ label }}</label>
            </div>
        }
    }

    fn draw_checkbox_with_label<F: Fn(bool) + 'static>(&self,
                                                       label: &str,
                                                       value: bool,
                                                       onchange: F)
                                                       -> Self::DrawResult {
        html! {
            <div>
                <input type="checkbox", checked=value, onclick=|_| { onchange(!value) ; Msg::Redraw }, />
                <label>{{ label }}</label>
            </div>
        }
    }

    fn draw_text_with_label(&self, text: &str, label: &str) -> Self::DrawResult {
        html! {
            <div>
                <p>{{ text }}</p>
                <label>{{ label }}</label>
            </div>
        }
    }

    fn draw_multiline_text_input_with_label<F: Fn(&str) -> () + 'static>(&self,
                                                                         label: &str,
                                                                         existing_value: &str,
                                                                         onchange: F)
                                                                         -> Self::DrawResult {
        html! {
            <div>
                <textarea rows=5, value=existing_value,
                          oninput=|e| { onchange(&e.value) ; Msg::Redraw }, >
                </textarea>
                <label>{{ label }}</label>
            </div>
        }
    }

    // TODO: wasm needs to call back into the app and tell it the window positions
    fn draw_window<F: Fn(Keypress) + 'static, G: Fn() + 'static, H>(&self,
                                                                    window_name: &str,
                                                                    size: (usize, usize),
                                                                    pos: (isize, isize),
                                                                    f: &dyn Fn()
                                                                            -> Self::DrawResult,
                                                                    handle_keypress: Option<F>,
                                                                    onclose: Option<G>,
                                                                    onwindowchange: H)
                                                                    -> Self::DrawResult
        where H: Fn((isize, isize), (usize, usize)) + 'static
    {
        // TODO: i should just be able to move onwindowchange... i wonder why we have to wrap it in
        // RC :/
        let onwindowchange = Rc::new(onwindowchange);
        let renderer_state = Rc::clone(&self.renderer_state);
        let run_after: Box<dyn Fn(&HtmlElement)> = Box::new(move |el| {
            let renderer_state = Rc::clone(&renderer_state);
            let onwindowchange = Rc::clone(&onwindowchange);
            let onwindowchange = move |target: stdweb::Value,
                                       pos_dx: stdweb::Value,
                                       pos_dy: stdweb::Value,
                                       new_width: stdweb::Value,
                                       new_height: stdweb::Value| {
                let el: HtmlElement = target.try_into().unwrap();

                let current_pos_x = num!(isize, js! { return parseFloat(@{&el}.style.left); });
                let current_pos_y = num!(isize, js! { return parseFloat(@{&el}.style.top); });
                let pos = (current_pos_x, current_pos_y);

                // newWidth and newHeight may be null if there's no change (if the window was
                // dragged, but not resized)
                let pos_d = (num!(isize, pos_dx), num!(isize, pos_dy));
                let new_pos = (pos.0 + pos_d.0, pos.1 + pos_d.1);
                let new_size = if new_width.is_null() && new_height.is_null() {
                    let current_width = num!(usize, js! { return parseFloat(@{&el}.style.width); });
                    let current_height =
                        num!(usize, js! { return parseFloat(@{&el}.style.height); });
                    (current_width, current_height)
                } else {
                    (num!(usize, new_width), num!(usize, new_height))
                };

                onwindowchange(new_pos, new_size);
                renderer_state.borrow().send_msg(Msg::Redraw);
            };

            js! { setupInteract(@{el}, @{onwindowchange}); };

            // auto-focus this new window on the first draw
            let dataset = el.dataset();
            let previous_draw_count = dataset.get("drawcount")
                                             .and_then(|s| s.parse::<isize>().ok())
                                             .unwrap_or(-1);
            if previous_draw_count == -1 {
                el.focus();
            }
            let next_draw_count = previous_draw_count + 1;
            dataset.insert("drawcount", next_draw_count.to_string().as_ref())
                   .ok();
        });

        // if there's a keypress handler provided, then send those keypresses into the app, and like,
        // prevent the tab key from doing anything
        let handle_keypress_1 = Rc::new(move |keypress: Keypress| {
            if let Some(handle_keypress) = &handle_keypress {
                handle_keypress(keypress)
            }
        });
        let handle_keypress_2 = Rc::clone(&handle_keypress_1);
        let global_keydown_handler = self.global_keydown_handler();
        // outline: none; prevents the browser from drawing the ring outline around active windows,
        // which we don't need because we already differentiate active windows w/ a different titlebar
        // bg color
        let window_style = format!(
            "border: 1px solid grey; outline: none !important; left: {}px; top: {}px; color: white; background-color: {}; width: {}px; height: {}px;",
            pos.0, pos.1, rgba(colorscheme!(window_bg_color)), size.0, size.1);
        run_after_render::run(html! {
                                 <div class="window",
                                      style={ window_style  },
                                      tabindex=0,
                                      onkeypress=|e| {
                                          if let Some(keypress) = map_keypress_event(&e) {
                                              handle_keypress_1(keypress);
                                          }
                                          e.prevent_default();
                                          Msg::Redraw
                                      },
                                      onkeydown=|e| {
                                          global_keydown_handler(&e);
                                          // lol for these special keys we have to listen on keydown, but the
                                          // rest we can do keypress :/
                                          if e.key() == "Tab" || e.key() == "Escape" || e.key() == "Esc" ||
                                              // LOL this is for ctrl+r
                                              ((e.key() == "r" || e.key() == "R") && e.ctrl_key()) {
                                              //console!(log, e.key());
                                              if let Some(keypress) = map_keypress_event(&e) {
                                                  //console!(log, format!("{:?}", keypress));
                                                  handle_keypress_2(keypress);
                                              }
                                              e.prevent_default();
                                              Msg::Redraw
                                          } else {
                                              Msg::DontRedraw
                                          }
                                      }, >

                                      // outline: none prevents browsers from drawing a border around the window when
                                      // it's selected. there's no need because we already differentiate active windows
                                      // with a different titlebar color
                                      { css(&format!(r#"
                                                .window-title {{ background-color: {}; }}
                                                .window:focus-within .window-title {{ background-color: {}; }}
                                            "#, rgba(colorscheme!(titlebar_bg_color)),
                                          rgba(colorscheme!(titlebar_active_bg_color))
                                      )) }

                                     <div class="window-title", style="color: white;",>
                                          { if let Some(onclose) = onclose {
                                              html! {
                                                  <div style="float: right; cursor: pointer;", onclick=|_| { onclose(); Msg::Redraw }, >
                                                      { symbolize_text("🗙") }
                                                  </div>
                                              }
                                          } else {
                                              html! { <div></div> }
                                          } }
                                          { window_name }
                                      </div>
                                      <div class="window-content",>
                                          { f() }
                                      </div>
                                  </div>
                              },
                              run_after)
    }

    // TODO: implement these
    fn draw_code_line_separator(&self,
                                plus_char: char,
                                width: f32,
                                height: f32,
                                color: Color)
                                -> Self::DrawResult {
        let line_offset = height / 2.;
        html! {
            <div style={ format!("position: relative; margin-top: 3px; margin-bottom: 4px; display: flex; width: {}px; height: {}px;", width, height)}, >
                <div style={ format!("color: {}; margin-top: -7.5px; z-index: 1;", rgba(color)) },>
                    { symbolize_text(&format!("{}", plus_char)) }
                </div>

                <div style={ format!("margin-top: {}px; background-color: {}; height: 1px; width: {}px;", line_offset, rgba(color), width) }, >
                    {" "}
                </div>
            </div>
        }
    }

    fn replace_on_hover(&self,
                        draw_when_not_hovered: &dyn Fn() -> Self::DrawResult,
                        draw_when_hovered: &dyn Fn() -> Self::DrawResult)
                        -> Self::DrawResult {
        let not_hovered_ref = NodeRef::default();
        let not_hovered_ref2 = not_hovered_ref.clone();
        let not_hovered_ref3 = not_hovered_ref.clone();
        let hovered_ref = NodeRef::default();
        let hovered_ref2 = hovered_ref.clone();
        let hovered_ref3 = hovered_ref.clone();
        // HAXXXX: ok this is insane, but the dom diffing engine in yew will mutate the hidden div
        // tags, and not reshow them when new stuff comes on the screen, and so we've gotta use replaceonhoverhack
        // tags instead. gonna define replaceonhoverhack to be display: block in the css file
        html! {
            <replaceonhoverhack class="fit-content", onmouseover=|_| { hide(&not_hovered_ref3) ; show(&hovered_ref3); Msg::DontRedraw },
                onmouseout=|_| { hide(&hovered_ref2) ; show(&not_hovered_ref2); Msg::DontRedraw }, >
                <replaceonhoverhack class="fit-content", ref={not_hovered_ref}, >
                    { draw_when_not_hovered() }
                </replaceonhoverhack>
                <replaceonhoverhack ref={hovered_ref}, style="display: none;", >
                    { draw_when_hovered() }
                </replaceonhoverhack>
            </replaceonhoverhack>
        }
    }

    // TODO: clean up bc code is duped between here and draw_window
    fn draw_child_region<F: Fn(Keypress) + 'static>(&self,
                                                    bg: Color,
                                                    draw_fn: &dyn Fn() -> Self::DrawResult,
                                                    height: ChildRegionHeight,
                                                    draw_context_menu: Option<&dyn Fn() -> Self::DrawResult>,
                                                    handle_keypress: Option<F>)
                                                    -> Self::DrawResult {
        // TODO: this is super duped from draw_window, clean this up already!!!! :((((
        // if there's a keypress handler provided, then send those keypresses into the app, and like,
        // prevent the tab key from doing anything
        let handle_keypress_1 = Rc::new(move |keypress: Keypress| {
            if let Some(handle_keypress) = &handle_keypress {
                handle_keypress(keypress)
            }
        });
        let handle_keypress_2 = Rc::clone(&handle_keypress_1);
        let global_keydown_handler = self.global_keydown_handler();

        // TODO: can we get all the context menu stuff from `context_menu`?
        let context_menu = draw_context_menu.map(|draw_context_menu| draw_context_menu());
        let is_context_menu = context_menu.is_some();

        let (container_css, height_css) = match height {
            // child regions don't have any vertical space before them... mirroring imgui
            ChildRegionHeight::ExpandFill { min_height } => {
                ("flex: 1; margin-top: 0px;",
                 format!("min-height: {}px; height: 100%;", min_height))
            }
            ChildRegionHeight::Pixels(px) => ("margin-top: 0px;", format!("height: {}px;", px)),
        };

        let context_menu_ref = NodeRef::default();
        let context_menu_ref2 = context_menu_ref.clone();
        let context_menu_trigger_ref = NodeRef::default();
        let context_menu_trigger_ref2 = context_menu_trigger_ref.clone();

        // TODO: border color is hardcoded, ripped from imgui
        // TODO: this isn't using the button_active color from the colorscheme
        html! {
            <div style={ container_css },>
                <div ref={context_menu_ref}, class="context_menu", style="display: none;",>
                    { context_menu.unwrap_or_else(|| VNode::from(VList::new())) }
                </div>

                <div style={ format!("border: 1px solid #6a6a6a; white-space: nowrap; background-color: {}; overflow: auto; {}", rgba(bg), height_css) },
                    ref={context_menu_trigger_ref},
                    tabindex=0,
                    class="context_menu_trigger",
                    oncontextmenu=|e| {
                        let context_menu_el : Element = (&context_menu_ref2).try_into().unwrap();
                        let context_menu_trigger_el : Element = (&context_menu_trigger_ref2).try_into().unwrap();
                        if is_context_menu {
                            e.prevent_default();
                            js! {
                                var e = @{&e};
                                @{show_right_click_menu}(@{context_menu_el}, @{context_menu_trigger_el}, e.clientX, e.clientY);
                            }
                        }
                        Msg::DontRedraw
                    },
                    onkeypress=|e| {
                        if let Some(keypress) = map_keypress_event(&e) {
                            handle_keypress_1(keypress);
                        }
                        e.prevent_default();
                        Msg::Redraw
                    },
                    onkeydown=|e| {
                        global_keydown_handler(&e);
                        // lol for these special keys we have to listen on keydown, but the
                        // rest we can do keypress :/
                        if e.key() == "Tab" || e.key() == "Escape" || e.key() == "Esc" ||
                            // LOL this is for ctrl+r
                            ((e.key() == "r" || e.key() == "R") && e.ctrl_key()) {
                            //console!(log, e.key());
                            if let Some(keypress) = map_keypress_event(&e) {
                                //console!(log, format!("{:?}", keypress));
                                handle_keypress_2(keypress);
                            }
                            e.prevent_default();
                            Msg::Redraw
                        } else {
                            Msg::DontRedraw
                        }
                    }, >

                    { draw_fn() }
                </div>
            </div>
        }
    }

    fn draw_layout_with_bottom_bar(&self,
                                   draw_content_fn: &dyn Fn() -> Self::DrawResult,
                                   draw_bottom_bar_fn: &dyn Fn() -> Self::DrawResult)
                                   -> Self::DrawResult {
        // TODO: this only renders the bottom bar directly under the content. the bottom bar needs
        // to be fixed at the bottom
        html! {
            <div style="display: flex; flex-direction: column;", >
                <div style="flex-grow: 1; display: flex;",>
                    { draw_content_fn() }
                </div>
                <div style="flex-grow: 0; display: flex;",>
                    { draw_bottom_bar_fn() }
                </div>
            </div>
        }
    }

    fn draw_empty_line(&self) -> Self::DrawResult {
        html! {
            <br />
        }
    }

    fn buttonize<F: Fn() + 'static>(&self,
                                    draw_fn: &dyn Fn() -> Self::DrawResult,
                                    onclick: F)
                                    -> Self::DrawResult {
        //        let draw_with_overlay_on_hover = draw_fn;
        let draw_with_overlay_on_hover = || {
            let mut drawn = vtag(draw_fn());
            // see buttonize-hover.js
            if drawn.attributes.contains_key("onmouseover") {
                panic!("{:?} already contains onmouseover", drawn);
            }
            let old_style = drawn.attributes
                                 .get("style")
                                 .map(|s| s.as_str())
                                 .unwrap_or("");
            let new_style = format!("{}; pointer-events: auto;", old_style);
            drawn.attributes.insert("style".into(), new_style);
            drawn.attributes.insert("onmouseover".into(),
                                    format!("displayButtonizedHoverOverlayOn(this, \"{}\");",
                                            rgba(colorscheme!(button_hover_color))));
            VNode::VTag(Box::new(drawn))
        };
        html! {
            <div style="position: relative; pointer-events: none;", onclick=|_| { onclick(); Msg::Redraw },
                 onmouseleave=|e| { js! { removeOverlays(@{e.target()}); } ; Msg::DontRedraw},>
                { draw_with_overlay_on_hover() }
                <div style="position: absolute; top: 0px; left: 0px; display: none; height: 0px; width: 0px; pointer-events: none;",
                     class="buttonized-hover-overlay",>
                     {" "}
                </div>

            </div>
        }
    }

    fn draw_buttony_text(&self, label: &str, color: [f32; 4]) -> Self::DrawResult {
        html! {
            <button class="fit-content",
                style=format!("color: white; background-color: {}; display: block; border: none; outline: none;", rgba(color)), >
                { symbolize_text(label) }
            </button>
        }
    }

    fn draw_button<F: Fn() + 'static>(&self,
                                      label: &str,
                                      color: [f32; 4],
                                      on_button_press_callback: F)
                                      -> Self::DrawResult {
        self.buttonize(&|| self.draw_buttony_text(label, color),
                       on_button_press_callback)
    }

    fn draw_small_button<F: Fn() + 'static>(&self,
                                            label: &str,
                                            color: [f32; 4],
                                            on_button_press_callback: F)
                                            -> Self::DrawResult {
        html! {
            <button style=format!("display: block; font-size: 75%; color: white; background-color: {}; border: none; outline: none;", rgba(color)),
                 onclick=|_| { on_button_press_callback(); Msg::Redraw }, >
            { label }
            </button>
        }
    }

    fn draw_text_box(&self, text: &str) -> Self::DrawResult {
        // this shit is the only way i can get a div to stay scrolled to the bottom
        // see https://stackoverflow.com/questions/18614301/keep-overflow-div-scrolled-to-bottom-unless-user-scrolls-up/44051405#44051405
        html! {
            <div style="height: 100%; overflow-y: auto; display: flex; flex-direction: column-reverse; border: none; width: 100%;",
                      readonly={true}, >
            { for text.lines().rev().into_iter().map(|line| html! {
                <div>{ symbolize_text(line) }</div>
            }) }
            </div>
        }
    }

    fn draw_all_on_same_line(&self,
                             draw_fns: &[&dyn Fn() -> Self::DrawResult])
                             -> Self::DrawResult {
        html! {
            <div style={"display: flex;"}, >
                { for draw_fns.into_iter().map(|draw_fn| html! {
                    <div>
                        { draw_fn() }
                    </div>
                })}
            </div>
        }
    }

    fn draw_text_input<F: Fn(&str) -> () + 'static, D: Fn() + 'static>(&self,
                                                                       existing_value: &str,
                                                                       onchange: F,
                                                                       ondone: D)
                                                                       -> Self::DrawResult {
        let ondone = Rc::new(ondone);
        let ondone2 = Rc::clone(&ondone);
        html! {
            <input type="text",
               style=format!("display: block; background-color: {};", rgba(colorscheme!(input_bg_color))),
               autocomplete="off",
               value=existing_value,
               oninput=|e| {onchange(&e.value) ; Msg::Redraw},
               onkeypress=|e| { if e.key() == "Enter" { ondone2() } ; Msg::Redraw }, />
        }
    }

    fn draw_whole_line_console_text_input(&self,
                                          ondone: impl Fn(&str) + 'static)
                                          -> Self::DrawResult {
        html! {
            <input type="text",
               style=format!("display: block; width: 100%; background-color: {}",
                             rgba(colorscheme!(input_bg_color))),
               autocomplete="off",
               value="",
               onkeypress=|e| {
                   if e.key() == "Enter" {
                     // no idea how to do this safely but it works!
                     let el : InputElement = unsafe { std::mem::transmute(e.target().unwrap()) };
                     ondone(&el.raw_value());
                     el.set_raw_value("");
                   }
                   Msg::Redraw
               }, />
        }
    }

    fn draw_text(&self, text: &str) -> Self::DrawResult {
        // forgot why we needed to do this, whoops, should've written a comment
        let text = text.replace(" ", " ");
        html! {
            <div style="padding: 0.2em;",>
                {
                    if text.is_empty() {
                        html! { <span>{" "}</span> }
                    } else {
                       symbolize_text(&text)
                    }
                }
            </div>
        }
    }

    fn draw_wrapped_text(&self, color: Color, text: &str) -> Self::DrawResult {
        html! {
            <div style=format!("padding: 0.2em; white-space: pre-wrap; word-wrap: break-word; color: {};", rgba(color)),>
                {
                    if text.is_empty() {
                        html! { <span>{" "}</span> }
                    } else {
                       symbolize_text(&text)
                    }
                }
            </div>
        }
    }

    // TODO: apparently this isn't needed in HTML, it happens automatically... though we needed it
    // in imgui
    fn draw_taking_up_full_width(&self, draw_fn: DrawFnRef<Self>) -> Self::DrawResult {
        html! {
            <div style="width: calc(100%); max-width: calc(100%);",>
                { draw_fn() }
            </div>
        }
    }

    fn draw_full_width_heading(&self,
                               bgcolor: Color,
                               inner_padding: (f32, f32),
                               text: &str)
                               -> Self::DrawResult {
        html! {
            <div style=format!("width: 100%; box-sizing: border-box; padding: 0.1em 0.35em; background-color: {}", rgba(bgcolor)),>
                <div style=format!("padding: {}px {}px", inner_padding.1 / 2., inner_padding.0 / 2.),>
                    { text }
                </div>
            </div>
        }
    }

    fn draw_with_margin(&self, margin: (f32, f32), draw_fn: DrawFnRef<Self>) -> Self::DrawResult {
        html! {
            <div style=format!("margin: {}px {}px", margin.1 / 2., margin.0 / 2.),>
                { draw_fn() }
            </div>
        }
    }

    fn focused(&self, draw_fn: &dyn Fn() -> Html<Model>) -> Self::DrawResult {
        run_after_render::run(draw_fn(), |element| {
            let el: &InputElement = unsafe { std::mem::transmute(element) };
            el.focus();
        })
    }

    // we're using droppy (https://github.com/OutlawPlz/droppy/) to help us render the dropdown.
    // TODO: the menu doesn't render correctly, and also doesn't go away when we select one of the
    // menu items
    fn draw_main_menu_bar(&self, draw_menus: &[DrawFnRef<Self>]) -> Self::DrawResult {
        run_after_render::run(html! {
                                  <nav class="dropdown-menu",
                                       style=format!("position: fixed; top: 0; left: 0; width: 100%; height: 1.25em; padding: 0.25em; background-color: {}; color: white; user-select: none;",
                                                     rgba(colorscheme!(menubar_color))), >
                                      {{ self.draw_all_on_same_line(draw_menus) }}
                                  </nav>
                              },
                              |el| {
                                  js! {
                                      var existingDroppyInstance = Droppy.prototype.getInstance(@{el});
                                      if (!existingDroppyInstance) {
                                          var droppy = new Droppy(@{el}, {
                                              parentSelector: ".main-menu-parent",
                                              dropdownSelector: ".main-menu-dropdown",
                                              triggerSelector: ".main-menu-label",
                                              closeOthers: true,
                                              clickOutToClose: true
                                          });
                                      }
                                  }
                              })
    }

    fn draw_menu(&self,
                 label: &str,
                 draw_menu_items: &dyn Fn() -> Self::DrawResult)
                 -> Self::DrawResult {
        html! {
            <div class="main-menu-parent">
                <div class="main-menu-label", style="padding: 1em; display: inline; cursor: default; margin: auto;", >
                    {label}
                </div>

                <ul class="main-menu-dropdown fit-content",
                    style="padding: 0.25em 1em; position: absolute; z-index: 99 !important; margin: 0; margin-top: 0.25em; background: black; opacity: 0.85;", >
                    {{ draw_menu_items() }}
                </ul>
            </div>
        }
    }

    fn draw_menu_item<F: Fn() + 'static>(&self, label: &str, onselect: F) -> Self::DrawResult {
        html! {
            <li style="list-style-type: none; margin: 0; padding: 0;",>
                {{ self.draw_button(label, BLACK_COLOR, move || {
                    // close open nav bar when a menu item is selected. the lib is supposed to do this
                    // for us, but yew's onclick handler prevents propagation :(
                    js! {
                        var el = document.querySelector(".dropdown-menu");
                        var existingDroppyInstance = Droppy.prototype.getInstance(el);
                        existingDroppyInstance.closeAll();
                    }
                    onselect();
                }) }}
            </li>
        }
    }

    fn draw_statusbar(&self, draw_fn: &dyn Fn() -> Self::DrawResult) -> Self::DrawResult {
        html! {
            <div style="position: fixed; line-height: 1; width: 100%; bottom: 0; left: 0;",>
                {{ draw_fn() }}
            </div>
        }
    }

    fn draw_combo_box_with_label<F, G, H, T>(&self,
                                             label: &str,
                                             is_item_selected: G,
                                             format_item: H,
                                             items: &[&T],
                                             onchange: F)
                                             -> Self::DrawResult
        where T: Clone + 'static,
              F: Fn(&T) -> () + 'static,
              G: Fn(&T) -> bool,
              H: Fn(&T) -> String
    {
        let formatted_items = items.into_iter().map(|i| format_item(i)).collect_vec();
        let selected_item_in_combo_box = items.into_iter().position(|i| is_item_selected(i));
        let items = items.into_iter().map(|i| (*i).clone()).collect_vec();
        html! {
            <div>
                <select onchange=|event| {
                            match event {
                                ChangeData::Select(elem) => {
                                    if let Some(selected_index) = elem.selected_index() {
                                        onchange(&items[selected_index as usize]);
                                    }
                                    Msg::Redraw
                                }
                                _ => {
                                    unreachable!();
                                }
                            }
                        },>
                    { for formatted_items.into_iter().enumerate().map(|(index, item)| {
                        let selected = Some(index) == selected_item_in_combo_box;
                        if selected {
                            html! {
                                <option selected=true, >
                                    { item }
                                </option>
                            }
                        } else {
                            html! {
                                <option>
                                    { item }
                                </option>
                            }
                        }
                    })}
                </select>
                <label>{ label }</label>
            </div>
        }
    }

    // TODO: make this NOT a total copy and paste of draw_combo_box_with_label
    fn draw_selectables<F, G, H, T>(&self,
                                    is_item_selected: G,
                                    format_item: H,
                                    items: &[&T],
                                    onchange: F)
                                    -> Self::DrawResult
        where T: Clone + 'static,
              F: Fn(&T) -> () + 'static,
              G: Fn(&T) -> bool,
              H: Fn(&T) -> &str
    {
        let formatted_items = items.into_iter().map(|i| format_item(i)).collect_vec();
        let selected_item_in_combo_box = items.into_iter().position(|i| is_item_selected(i));
        let items = items.into_iter().map(|i| (*i).clone()).collect_vec();
        html! {
            <select size={items.len().to_string()}, onchange=|event| {
                        match event {
                            ChangeData::Select(elem) => {
                                if let Some(selected_index) = elem.selected_index() {
                                    onchange(&items[selected_index as usize]);
                                }
                                Msg::Redraw
                            }
                            _ => {
                                unreachable!();
                            }
                        }
                    },>
                { for formatted_items.into_iter().enumerate().map(|(index, item)| {
                    let selected = Some(index) == selected_item_in_combo_box;
                    if selected {
                        html! {
                            <option selected=true, >
                                { item }
                            </option>
                        }
                    } else {
                        html! {
                            <option>
                                { item }
                            </option>
                        }
                    }
                })}
            </select>
        }
    }

    fn draw_selectables2<T, F: Fn(&T) -> () + 'static>(&self,
                                                       items: Vec<SelectableItem<T>>,
                                                       onselect: F)
                                                       -> Self::DrawResult {
        let items = Rc::new(items);
        let items_rc = Rc::clone(&items);
        html! {
            <select style="overflow: hidden;", size={items_rc.len().to_string()}, onchange=|event| {
                match event {
                        ChangeData::Select(elem) => {
                            if let Some(selected_index) = elem.selected_index() {
                                match &items_rc[selected_index as usize] {
                                    SelectableItem::Selectable { item, .. } => onselect(item),
                                    SelectableItem::GroupHeader(_) => panic!("expected a selectable here"),
                                }
                            }
                            Msg::Redraw
                        }
                        _ => {
                            unreachable!();
                        }
                    }
            },>
                { for items.iter().map(|item| match item {
                    SelectableItem::Selectable { label, is_selected, .. } => {
                        if *is_selected {
                            html! {
                                <option selected=true, >
                                    { label }
                                </option>
                            }
                        } else {
                            html! {
                                <option>
                                    { label }
                                </option>
                            }
                        }
                    },
                    SelectableItem::GroupHeader(label) => {
                        html! {
                            <option disabled=true, >
                                { label }
                            </option>
                        }
                    }

                }) }
            </select>
        }
    }

    fn draw_box_around(&self,
                       color: [f32; 4],
                       draw_fn: &dyn Fn() -> Self::DrawResult)
                       -> Self::DrawResult {
        // pointer-events stuff is to allow children to respond to click handlers
        // see https://stackoverflow.com/questions/3680429/click-through-div-to-underlying-elements
        html! {
            <div style="pointer-events: none;", class="overlay-wrapper", >
                <div style="pointer-events: auto;", >
                     { draw_fn() }
                 </div>
                 <div class="overlay",
                      style={ format!("pointer-events: none; top: 0px; left: 0px; height: 100%; background-color: {};", rgba(color)) }, >
                      {" "}
                 </div>
             </div>
        }
    }

    fn draw_top_border_inside(&self,
                              color: [f32; 4],
                              thickness: u8,
                              draw_fn: &dyn Fn() -> Self::DrawResult)
                              -> Self::DrawResult {
        html! {
            <div class={"overlay-wrapper"}, >
                <div>
                     { draw_fn() }
                 </div>
                 <div class={"overlay"},
                      style={ format!("height: {}px; background-color: {}", thickness, rgba(color)) }, >
                      {" "}
                 </div>
             </div>
        }
    }

    fn draw_right_border_inside(&self,
                                color: [f32; 4],
                                thickness: u8,
                                draw_fn: &dyn Fn() -> Self::DrawResult)
                                -> Self::DrawResult {
        html! {
            <div class={"overlay-wrapper"}, >
                <div>
                     { draw_fn() }
                 </div>
                 <div class={"overlay-bottom-right"},
                      style={ format!("height: 100%; width: {}px; background-color: {}", thickness, rgba(color)) }, >
                      {" "}
                 </div>
             </div>
        }
    }

    fn draw_left_border_inside(&self,
                               color: [f32; 4],
                               thickness: u8,
                               draw_fn: &dyn Fn() -> Self::DrawResult)
                               -> Self::DrawResult {
        html! {
            <div class={"overlay-wrapper"}, >
                <div>
                     { draw_fn() }
                 </div>
                 <div class={"overlay"},
                      style={ format!("height: 100%; width: {}px; background-color: {}", thickness, rgba(color)) }, >
                      {" "}
                 </div>
             </div>
        }
    }

    fn draw_bottom_border_inside(&self,
                                 color: [f32; 4],
                                 thickness: u8,
                                 draw_fn: &dyn Fn() -> Self::DrawResult)
                                 -> Self::DrawResult {
        html! {
            <div class={"overlay-wrapper"}, >
                <div>
                     { draw_fn() }
                 </div>
                 <div class={"overlay-bottom-right"},
                      style={ format!("width: 100%; height: {}px; background-color: {}", thickness, rgba(color)) }, >
                      {" "}
                 </div>
             </div>
        }
    }

    fn indent(&self, px: i16, draw_fn: &dyn Fn() -> Self::DrawResult) -> Self::DrawResult {
        html! {
            <div style=format!("margin-left: {}px", px), >
                { draw_fn() }
            </div>
        }
    }

    fn align(&self,
             lhs: &dyn Fn() -> Self::DrawResult,
             rhs: &[&dyn Fn() -> Self::DrawResult])
             -> Self::DrawResult {
        html! {
            <div>
                <div style={"display: inline-block; vertical-align: top;"},>
                    { lhs() }
                </div>
                <div style={"display: inline-block; vertical-align: top;"}, >
                    { for rhs.into_iter().map(|draw_fn| html! {
                        { draw_fn() }
                    })}
                </div>
            </div>
        }
    }

    fn handle_global_keypress(&self, handle_keypress: impl Fn(Keypress) + 'static) {
        self.renderer_state
            .borrow()
            .set_global_keydown_handler(handle_keypress);
    }

    fn draw_with_bgcolor(&self, bgcolor: Color, draw_fn: DrawFnRef<Self>) -> Self::DrawResult {
        html! {
            <div style=format!("background-color: {};", rgba(bgcolor)),>
                { draw_fn() }
            </div>
        }
    }

    fn draw_with_no_spacing_afterwards(&self, draw_fn: DrawFnRef<Self>) -> Self::DrawResult {
        // i think we automatically do this
        draw_fn()
    }

    fn context_menu(&self,
                    draw_fn: DrawFnRef<Self>,
                    draw_context_menu: DrawFnRef<Self>)
                    -> Self::DrawResult {
        let context_menu_ref = NodeRef::default();
        let context_menu_ref2 = context_menu_ref.clone();
        let context_menu_trigger_ref = NodeRef::default();
        let context_menu_trigger_ref2 = context_menu_trigger_ref.clone();
        html! {
            <div>
                <div ref={context_menu_ref}, class="context_menu", style="display: none;",>
                    { draw_context_menu() }
                </div>

                <div ref={context_menu_trigger_ref}, class="context_menu_trigger", oncontextmenu=|e| {
                    let context_menu_el : Element = (&context_menu_ref2).try_into().unwrap();
                    let context_menu_trigger_el : Element = (&context_menu_trigger_ref2).try_into().unwrap();
                    e.prevent_default();
                    e.stop_propagation();
                    js! {
                        var e = @{&e};
                        @{show_right_click_menu}(@{context_menu_el}, @{context_menu_trigger_el}, e.clientX, e.clientY);
                    }
                    Msg::DontRedraw
                }, >
                    {{ draw_fn() }}
                </div>
            </div>
        }
    }
}

impl YewToolkit {
    fn new(renderer_state: Rc<RefCell<RendererState>>) -> Self {
        YewToolkit { renderer_state }
    }

    fn global_keydown_handler(&self) -> impl Fn(&KeyDownEvent) + 'static {
        let renderer_state = Rc::clone(&self.renderer_state);
        move |e| {
            renderer_state.borrow().handle_global_key(e);
        }
    }
}

pub struct RendererState {
    pub global_key_handler: Rc<RefCell<Box<dyn Fn(Keypress) + 'static>>>,
    pub yew_app: Rc<RefCell<html::Scope<Model>>>,
}

impl RendererState {
    pub fn new(yew_app: html::Scope<Model>) -> Self {
        Self { global_key_handler: Rc::new(RefCell::new(Box::new(|_| {}))),
               yew_app: Rc::new(RefCell::new(yew_app)) }
    }

    pub fn send_msg(&self, msg: Msg) {
        self.yew_app.borrow_mut().send_message(msg);
    }

    pub fn handle_global_key(&self, e: &KeyDownEvent) {
        // TODO: we know we have to capture C-o here because it can open the fuzzy finder
        // globally. unfortunately, for now, we have to manually bind all global hotkeys
        // like this.
        if (e.key() == "o" || e.key() == "O") && e.ctrl_key() {
            if let Some(keypress) = map_keypress_event(e) {
                (self.global_key_handler.borrow())(keypress);
                e.prevent_default();
                self.send_msg(Msg::Redraw)
            }
        }
    }

    #[allow(unused_must_use)]
    pub fn set_global_keydown_handler(&self, handle_keypress: impl Fn(Keypress) + 'static) {
        self.global_key_handler.replace(Box::new(handle_keypress));
    }
}

fn map_keypress_event<F: IKeyboardEvent>(keypress_event: &F) -> Option<Keypress> {
    let keystring_from_event = keypress_event.key();
    let appkey = map_key(&keystring_from_event)?;
    let was_shift_pressed =
        keypress_event.shift_key() || was_shift_key_pressed(&keystring_from_event);
    Some(Keypress::new(appkey, keypress_event.ctrl_key(), was_shift_pressed))
}

fn map_key(key: &str) -> Option<AppKey> {
    match key.to_lowercase().as_ref() {
        "a" => Some(AppKey::A),
        "b" => Some(AppKey::B),
        "c" => Some(AppKey::C),
        "h" => Some(AppKey::H),
        "j" => Some(AppKey::J),
        "k" => Some(AppKey::K),
        "l" => Some(AppKey::L),
        "d" => Some(AppKey::D),
        "w" => Some(AppKey::W),
        "x" => Some(AppKey::X),
        "r" => Some(AppKey::R),
        "o" => Some(AppKey::O),
        "u" => Some(AppKey::U),
        "v" => Some(AppKey::V),
        "tab" => Some(AppKey::Tab),
        "arrowleft" => Some(AppKey::LeftArrow),
        "arrowright" => Some(AppKey::RightArrow),
        "arrowup" => Some(AppKey::UpArrow),
        "arrowdown" => Some(AppKey::DownArrow),
        "esc" | "escape" => Some(AppKey::Escape),
        _ => None,
    }
}

fn was_shift_key_pressed(key: &str) -> bool {
    key.len() == 1 && key.chars().next().unwrap().is_uppercase()
}

pub fn draw_app(app: Rc<RefCell<CSApp>>, mut async_executor: AsyncExecutor) {
    yew::initialize();

    // add css styles referencing code that we wouldn't be able to access from .css files
    //    add_style_string(&format!(".buttonized {{ background-color: {}; }}",
    //                              rgba(COLOR_SCHEME.button_hover_color)));

    let yew_app = App::<Model>::new().mount_to_body();
    let renderer_state = Rc::new(RefCell::new(RendererState::new(yew_app)));

    setup_ui_update_on_io_event_completion(&mut async_executor, Rc::clone(&renderer_state));
    add_global_keydown_event_listener(Rc::clone(&renderer_state));
    renderer_state.borrow().send_msg(Msg::Init(Rc::clone(&app),
                                               async_executor.clone(),
                                               Rc::clone(&renderer_state)));
    yew::run_loop();
}

fn setup_ui_update_on_io_event_completion(async_executor: &mut AsyncExecutor,
                                          renderer_state: Rc<RefCell<RendererState>>) {
    async_executor.setonupdate(Rc::new(move || {
                                   renderer_state.borrow().send_msg(Msg::Redraw);
                               }));
}

fn add_global_keydown_event_listener(renderer_state: Rc<RefCell<RendererState>>) {
    document().add_event_listener(move |e: KeyDownEvent| {
                  renderer_state.borrow().handle_global_key(&e);
              });
}

fn symbolize_text(text: &str) -> Html<Model> {
    html! {
        <span>
            { for text.chars().map(|char| {
                if is_in_symbol_range(char) {
                    html! {
                        <span style="display: inline-block; font-size: 57%; transform: translateY(-1px);",>
                          { char }
                        </span>
                    }
                } else {
                    html! {
                        <span>{ char }</span>
                    }
                }
            })}
        </span>
    }
}

fn is_in_symbol_range(c: char) -> bool {
    match c as u32 {
        0xf000..=0xf72f => true,
        _ => false,
    }
}

fn show(node_ref: &NodeRef) {
    let element = node_ref.try_into::<Element>().unwrap();
    js! {
        let el = @{element};
        if (el) {
            el.style.display = "block";
        }
    }
}

fn hide(node_ref: &NodeRef) {
    let element = node_ref.try_into::<Element>().unwrap();
    js! {
        let el = @{element};
        if (el) {
            el.style.display = "none";
        }
    }
}

// from https://stackoverflow.com/a/15506705/149987, adapted to stdweb
// don't need it right now but might need it later
//fn add_style_string(css_str: &str) {
//    let node = document().create_element("style").unwrap();
//    node.set_attribute("innerHTML", css_str).unwrap();
//    document().body().unwrap().append_child(&node);
//}

fn rgba(color: [f32; 4]) -> String {
    format!("rgba({}, {}, {}, {})",
            color[0] * 255.0,
            color[1] * 255.0,
            color[2] * 255.0,
            color[3])
}

fn vtag(html: Html<Model>) -> VTag<Model> {
    match html {
        VNode::VTag(box vtag) => vtag,
        _ => panic!("expected a vtag!"),
    }
}

use stdweb::unstable::TryFrom;
use stdweb::web::Node;

fn css(css: &str) -> Html<Model> {
    raw_html(&format!(
        r#"
        <style type="text/css">
            {}
        </style>
    "#,
        css
    ))
}

//badboy from https://github.com/yewstack/yew/blob/master/examples/inner_html/src/lib.rs
fn raw_html(raw_html: &str) -> Html<Model> {
    let js_el = js! {
        var div = document.createElement("div");
        div.innerHTML = @{raw_html};
        return div;
    };
    let node = Node::try_from(js_el).expect("convert js_el");
    VNode::VRef(node)
}

fn show_right_click_menu(el1: stdweb::Value,
                         el2: stdweb::Value,
                         page_x: stdweb::Value,
                         page_y: stdweb::Value) {
    let page_x = num!(i32, page_x);
    let page_y = num!(i32, page_y);
    let context_menu_el: Element = el1.try_into().unwrap();
    let context_menu_trigger_el: Element = el2.try_into().unwrap();
    js! {
        showRightClickMenu(@{&context_menu_el}, @{&context_menu_trigger_el}, @{&page_x}, @{&page_y});
    };
}
