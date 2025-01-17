// Copyright 2020-2022 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

use std::cell::{BorrowMutError, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use js_sys::Error;
use luminance_web_sys::WebSysWebGL2Surface;
use rand::{thread_rng, Rng as _};
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::{wasm_bindgen, Closure, JsValue};
use wasm_bindgen::JsCast; // dyn_into()
use web_sys::{
    console, AddEventListenerOptions, Document, Element, Event, FocusEvent, HtmlElement,
    HtmlProgressElement, KeyboardEvent, MouseEvent, Text, WebGlContextAttributes,
};
use winit::platform::web::WindowBuilderExtWebSys as _;

use all_is_cubes::apps::{Key, Session, StandardCameras};
use all_is_cubes::camera::Viewport;
use all_is_cubes::cgmath::{Point2, Vector2};
use all_is_cubes::listen::ListenableCell;
use all_is_cubes::universe::UniverseStepInfo;
use all_is_cubes::util::YieldProgress;
use all_is_cubes_gpu::in_luminance;
use all_is_cubes_gpu::in_wgpu;

use crate::js_bindings::GuiHelpers;
use crate::url_params::{options_from_query_string, OptionsInUrl, RendererOption};
use crate::web_glue::{
    add_event_listener, get_mandatory_element, replace_children_with_one_text_node,
    yield_to_event_loop,
};

#[allow(clippy::large_enum_variant)]
enum WebRenderer {
    Luminance(in_luminance::SurfaceRenderer<WebSysWebGL2Surface>),
    Wgpu(in_wgpu::SurfaceRenderer),
}

/// Entry point for normal game-in-a-web-page operation.
#[wasm_bindgen]
pub async fn start_game(gui_helpers: GuiHelpers) -> Result<(), JsValue> {
    // Note: This used to be in a `#[wasm_bindgen(start)]` function, but that stopped working.
    // Rather than stop to figure out what went wrong even though I Didn't Change Anything,
    // I moved it here since this is our sole entry point in practice.
    console_error_panic_hook::set_once();

    // Initialize logging via the `log` crate's interface.
    // We use `console_log` to perform the actual logging, but it doesn't offer a message source
    // filter, so we have to do that ourselves.
    log::set_logger({
        struct FilteredWebLogger;
        impl log::Log for FilteredWebLogger {
            fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                let t = metadata.target();
                // Trace is the finest level, so no need to check it
                /* metadata.level() <= log::LevelFilter::Trace && */
                !t.starts_with("wgpu") && !t.starts_with("winit") && !t.starts_with("naga")
            }
            fn log(&self, record: &log::Record<'_>) {
                if self.enabled(record.metadata()) {
                    console_log::log(record);
                }
            }
            fn flush(&self) {}
        }
        &FilteredWebLogger
    })
    .unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let document = web_sys::window()
        .expect("missing `window`")
        .document()
        .expect("missing `document`");

    // TODO: StaticDom and GuiHelpers are the same kind of thing. Merge them?
    let static_dom = StaticDom::new(&document)?;
    {
        let list = static_dom.app_root.class_list();
        list.remove_1("state-script-not-loaded").unwrap();
        list.add_1("state-loading").unwrap();
    }

    let progress = YieldProgress::new(yield_to_event_loop, {
        let progress_bar = SendWrapper::new(static_dom.progress_bar.clone());
        move |fraction| progress_bar.set_value(fraction.into())
    });
    let [app_progress, progress] = progress.split(0.1);
    let [universe_progress, post_universe_progress] = progress.split(0.98);

    let OptionsInUrl {
        template,
        graphics_options,
        renderer: renderer_option,
    } = options_from_query_string(
        document
            .location()
            .unwrap()
            .search()?
            .trim_start_matches('?')
            .as_bytes(),
    );

    static_dom
        .loading_log
        .append_data("\nInitializing application...")?;
    app_progress.progress(0.2).await;
    // The main cost of this is constructing the `Vui` instance.
    // TODO: pipe in YieldProgress
    let session = Session::new().await;
    session.graphics_options_mut().set(graphics_options);

    static_dom
        .loading_log
        .append_data("\nInitializing graphics...")?;
    app_progress.progress(0.4).await;

    let viewport_cell = ListenableCell::new(gui_helpers.canvas_helper().viewport());
    let cameras = StandardCameras::from_session(&session, viewport_cell.as_source()).unwrap();
    let renderer = match renderer_option {
        RendererOption::Luminance => {
            let surface = WebSysWebGL2Surface::from_canvas_with_params(
                web_sys::window().unwrap(), // TODO messy
                document,
                gui_helpers.canvas_helper().canvas(),
                // This is set for parity with the all-is-cubes-desktop defaults.
                // TODO: Probably `GraphicsOptions` should get an antialias/MSAA field.
                WebGlContextAttributes::new().antialias(false),
            )
            .map_err(|e| Error::new(&format!("did not initialize WebGL: {}", e)))?;

            let renderer = in_luminance::SurfaceRenderer::new(surface, cameras)
                .map_err(|e| Error::new(&format!("did not initialize renderer: {}", e)))?;

            WebRenderer::Luminance(renderer)
        }
        RendererOption::Wgpu => {
            let event_loop = winit::event_loop::EventLoop::new(); // note: discarding this, hopefully harmlessly
            let canvas = gui_helpers.canvas_helper().canvas();
            let winit_window = winit::window::WindowBuilder::new()
                .with_canvas(Some(canvas.clone()))
                .build(&event_loop)
                .unwrap();
            // Undo the things winit did
            let _ = canvas.style().remove_property("width");
            let _ = canvas.style().remove_property("height");
            gui_helpers.canvas_helper().update_viewport();

            let wgpu_instance = wgpu::Instance::new(wgpu::Backends::all());
            // Safety: create_surface specifies that the "window" must be kept alive
            // as long as the surface is. In this case, that's the canvas element, which we
            // will never remove (and in any case, almost certainly isn't going to cause UB).
            let surface = unsafe { wgpu_instance.create_surface(&winit_window) };
            // TODO: we lost the 'request no MSAA' feature
            let adapter = wgpu_instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .ok_or_else(|| {
                    Error::new("Could not request suitable graphics adapter")
                })?;
            let renderer = in_wgpu::SurfaceRenderer::new(cameras, surface, &adapter)
                .await
                .map_err(|e| Error::new(&format!("did not initialize GPU: {}", e)))?;
            WebRenderer::Wgpu(renderer)
        }
    };

    static_dom
        .loading_log
        .append_data("\nStarting game loop...")?;
    app_progress.progress(0.8).await;
    let root = WebGameRoot::new(
        gui_helpers,
        static_dom.clone(),
        session,
        renderer,
        viewport_cell,
    );
    root.borrow().start_loop();

    static_dom
        .loading_log
        .append_data("\nConstructing universe...")?;
    app_progress.progress(1.0).await;
    let universe = template
        .build(universe_progress, thread_rng().gen())
        .await
        .expect("universe template error");
    root.borrow_mut().session.set_universe(universe);

    // Explicitly keep the game loop alive.
    Box::leak(Box::new(root));

    // Do the final UI cleanup going from "loading" to "running".
    post_universe_progress.progress(1.0).await;
    {
        // TODO: make this part the WebGameRoot's responsibility? Move the class list manip to StaticDom?
        let list = static_dom.app_root.class_list();
        list.remove_1("state-loading").unwrap();
        list.add_1("state-fully-loaded").unwrap();
    }
    console::log_1(&JsValue::from_str("start_game() completed."));
    static_dom.loading_log.set_data("");
    Ok(())
}

struct WebGameRoot {
    /// In order to be able to set up callbacks to ourselves, we need to live in a mutable
    /// heap-allocated location, and we need to have a reference to that location. In
    /// order to not be a guaranteed memory leak, we need that reference to be weak.
    ///
    /// This technique taken from [this example of how to build a requestAnimationFrame
    /// loop](https://rustwasm.github.io/docs/wasm-bindgen/examples/request-animation-frame.html).
    self_ref: Weak<RefCell<WebGameRoot>>,

    gui_helpers: GuiHelpers,
    static_dom: StaticDom,
    session: Session,
    renderer: WebRenderer,
    viewport_cell: ListenableCell<Viewport>,
    raf_callback: Closure<dyn FnMut(f64)>,
    step_callback: Closure<dyn FnMut()>,
    step_callback_scheduled: bool,
    last_raf_timestamp: f64,
    last_step_info: UniverseStepInfo,
}

impl WebGameRoot {
    pub fn new(
        gui_helpers: GuiHelpers,
        static_dom: StaticDom,
        session: Session,
        renderer: WebRenderer,
        viewport_cell: ListenableCell<Viewport>,
    ) -> Rc<RefCell<WebGameRoot>> {
        // Construct a non-self-referential initial mutable object.
        let self_cell_ref = Rc::new(RefCell::new(Self {
            self_ref: Weak::new(),

            gui_helpers,
            static_dom,
            session,
            renderer,
            viewport_cell,
            raf_callback: Closure::wrap(Box::new(|_| { /* dummy no-op for initialization */ })),
            step_callback: Closure::wrap(Box::new(|| { /* dummy no-op for initialization */ })),
            step_callback_scheduled: false,
            last_raf_timestamp: 0.0, // TODO better initial value or special case
            last_step_info: UniverseStepInfo::default(),
        }));

        // Add the self-references.
        {
            let mut self_mut = (*self_cell_ref).borrow_mut();
            self_mut.self_ref = Rc::downgrade(&self_cell_ref);

            let weak_self_ref = self_mut.self_ref.clone();
            self_mut.raf_callback = Closure::wrap(Box::new(move |dom_timestamp: f64| {
                Self::upgrade_in_callback(&weak_self_ref, move |this| {
                    this.raf_callback_impl(dom_timestamp)
                })
            }));

            let weak_self_ref = self_mut.self_ref.clone();
            self_mut.step_callback = Closure::wrap(Box::new(move || {
                Self::upgrade_in_callback(&weak_self_ref, |this| this.step_callback_impl())
            }));
        }
        // Other initialization.
        (*self_cell_ref).borrow().init_dom();

        self_cell_ref
    }

    /// This method is broken out of new() so we can just use `self`. Well, some of the time.
    fn init_dom(&self) {
        self.add_canvas_to_self_event_listener(
            "keydown",
            false,
            move |this, event: KeyboardEvent| {
                if let Some(key) = map_keyboard_event(&event) {
                    this.session.input_processor.key_down(key);

                    // TODO: return for keys we don't bind
                    let event: &Event = event.as_ref();
                    event.stop_propagation(); // only if we didn't return already
                    event.prevent_default();
                }
            },
        );

        self.add_canvas_to_self_event_listener(
            "keyup",
            false,
            move |this, event: KeyboardEvent| {
                if let Some(key) = map_keyboard_event(&event) {
                    this.session.input_processor.key_up(key);

                    // TODO: return for keys we don't bind
                    let event: &Event = event.as_ref();
                    event.stop_propagation(); // only if we didn't return already
                    event.prevent_default();
                }
            },
        );

        self.add_canvas_to_self_event_listener("focus", true, move |this, _: FocusEvent| {
            this.session.input_processor.key_focus(true);
        });

        self.add_canvas_to_self_event_listener("blur", true, move |this, _: FocusEvent| {
            this.session.input_processor.key_focus(false);
            this.session.input_processor.mouse_ndc_position(None);
        });

        self.add_canvas_to_self_event_listener(
            "mousemove",
            true,
            move |this, event: MouseEvent| {
                this.update_mouse_position(&event);
            },
        );

        self.add_canvas_to_self_event_listener(
            "mouseover",
            true,
            move |this, event: MouseEvent| {
                this.update_mouse_position(&event);
            },
        );

        self.add_canvas_to_self_event_listener("mouseout", true, move |this, _: MouseEvent| {
            this.session.input_processor.mouse_ndc_position(None);
        });

        self.add_canvas_to_self_event_listener(
            "mousedown",
            true,
            move |this, event: MouseEvent| {
                this.update_mouse_position(&event);
                // MouseEvent button numbering is sequential for a three button mouse, instead of
                // counting the middle/wheel button as the third button.
                let mapped_button: usize = match event.button() {
                    0 => 0,
                    2 => 1,
                    1 => 2,
                    x => x as usize,
                };
                this.session.click(mapped_button);
            },
        );

        add_event_listener(
            &self.gui_helpers.canvas_helper().canvas(),
            "contextmenu",
            move |event: MouseEvent| {
                // Inhibits context menu so that we can use right-click as a game action.
                event.prevent_default();
            },
            &AddEventListenerOptions::new(),
        );
    }

    fn add_canvas_to_self_event_listener<E, F>(&self, event_name: &str, passive: bool, callback: F)
    where
        E: JsCast,
        F: Fn(&mut Self, E) + 'static,
    {
        let weak_self_ref = self.self_ref.clone();
        add_event_listener(
            &self.gui_helpers.canvas_helper().canvas(),
            event_name,
            move |event: E| {
                Self::upgrade_in_callback(&weak_self_ref, |this| callback(&mut *this, event))
            },
            AddEventListenerOptions::new().passive(passive),
        );
    }

    fn upgrade_in_callback<F>(weak_self_ref: &Weak<RefCell<WebGameRoot>>, body: F)
    where
        F: FnOnce(&mut Self),
    {
        if let Some(strong_self_ref) = weak_self_ref.upgrade() {
            match strong_self_ref.try_borrow_mut() {
                Ok(mut this) => body(&mut *this),
                Err(BorrowMutError { .. }) => {
                    // We probably left the cell borrowed in a previous panic.
                    // Log, but don't panic again because it will only create log spam.
                    console::warn_1(&JsValue::from_str(
                        "WebGameRoot is borrowed at event handler (check previous errors)",
                    ));
                }
            }
        } else {
            // Weak reference is dead; nothing to do.
            // TODO: We could unregister this callback.
            // (But ideally that would be done _on_ drop, not lazily.)
        }
    }

    pub fn start_loop(&self) {
        // This strategy from https://rustwasm.github.io/docs/wasm-bindgen/examples/request-animation-frame.html
        web_sys::window()
            .unwrap()
            .request_animation_frame(self.raf_callback.as_ref().unchecked_ref())
            .unwrap();
    }

    fn raf_callback_impl(&mut self, dom_timestamp: f64) {
        let delta = Duration::from_secs_f64((dom_timestamp - self.last_raf_timestamp) / 1000.0);
        self.last_raf_timestamp = dom_timestamp;
        let should_draw = self.session.frame_clock.request_frame(delta);

        if should_draw {
            let viewport = self.gui_helpers.canvas_helper().viewport();
            if viewport != *self.viewport_cell.get() {
                self.viewport_cell.set(viewport);
            }
            // TODO: assist this using common traits
            match &mut self.renderer {
                WebRenderer::Luminance(renderer) => {
                    renderer.objects.update_world_camera();
                    self.session.update_cursor(renderer.objects.cameras());
                }
                WebRenderer::Wgpu(renderer) => {
                    renderer.update_world_camera();
                    self.session.update_cursor(renderer.cameras());
                }
            }

            // Do graphics
            let render_info = match &mut self.renderer {
                WebRenderer::Luminance(renderer) => renderer
                    .render_frame(self.session.cursor_result())
                    .expect("error in render_frame"),
                WebRenderer::Wgpu(renderer) => {
                    // note: info text is HTML on web, so no string passed here
                    renderer
                        .render_frame(self.session.cursor_result(), |_| String::new())
                        .expect("error in render_frame")
                }
            };

            // Update info text
            let cameras: &StandardCameras = match &self.renderer {
                WebRenderer::Luminance(renderer) => renderer.objects.cameras(),
                WebRenderer::Wgpu(renderer) => renderer.cameras(),
            };
            if cameras.cameras().world.options().debug_info_text {
                self.static_dom
                    .scene_info_text_node
                    .set_data(&format!("{}", self.session.info_text(render_info)));
            } else {
                self.static_dom.scene_info_text_node.set_data("");
            }
        }

        if self.session.frame_clock.should_step() && !self.step_callback_scheduled {
            self.step_callback_scheduled = true;
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    self.step_callback.as_ref().unchecked_ref(),
                    0,
                )
                .unwrap();
        }

        // Sync pointer lock state
        let wants = self.session.input_processor.wants_pointer_lock();
        let has = self.check_pointer_lock();
        if wants != has {
            let canvas = self.gui_helpers.canvas_helper().canvas();
            if wants {
                canvas.request_pointer_lock();
            } else if let Some(document) = canvas.owner_document() {
                document.exit_pointer_lock();
            }
        }

        // Schedule next requestAnimationFrame
        self.start_loop();
    }

    fn step_callback_impl(&mut self) {
        self.step_callback_scheduled = false;
        if let Some(universe_step_info) = self.session.maybe_step_universe() {
            self.last_step_info = universe_step_info;
        }
    }

    fn update_mouse_position(&mut self, event: &MouseEvent) {
        let lock = self.check_pointer_lock();

        let i = &mut self.session.input_processor;
        i.mouselook_delta(Vector2::new(
            event.movement_x().into(),
            event.movement_y().into(),
        ));
        i.has_pointer_lock(lock);
        i.mouse_pixel_position(
            *self.viewport_cell.get(),
            Some(Point2::new(
                event.client_x().into(),
                event.client_y().into(),
            )),
            false,
        );
    }

    fn check_pointer_lock(&self) -> bool {
        let canvas = self.gui_helpers.canvas_helper().canvas(); // TODO: less indirection?
        canvas
            .owner_document()
            .and_then(|d| d.pointer_lock_element())
            == canvas.dyn_into::<Element>().ok()
    }
}

#[derive(Clone, Debug)]
struct StaticDom {
    /// The highest-level element we're supposed to touch, used for setting CSS classes.
    /// Usually the document element.
    app_root: HtmlElement,
    progress_bar: HtmlProgressElement,
    loading_log: Text,
    scene_info_text_node: Text,
}

impl StaticDom {
    fn new(document: &Document) -> Result<Self, Error> {
        Ok(Self {
            app_root: get_mandatory_element(document, "app-root")?,
            progress_bar: get_mandatory_element(document, "loading-progress-bar")?,
            loading_log: replace_children_with_one_text_node(get_mandatory_element(
                document,
                "loading-log",
            )?),
            scene_info_text_node: replace_children_with_one_text_node(get_mandatory_element(
                document,
                "scene-info-text",
            )?),
        })
    }
}

fn map_keyboard_event(event: &KeyboardEvent) -> Option<Key> {
    if event.alt_key() || event.ctrl_key() || event.meta_key() {
        return None;
    }
    Some(match event.key_code() as u8 as char {
        '\x25' => Key::Left,
        '\x26' => Key::Up,
        '\x27' => Key::Right,
        '\x28' => Key::Down,
        c @ '\x20'..='\x7e' => Key::Character(c.to_ascii_lowercase()),
        _ => {
            return None;
        }
    })
}
