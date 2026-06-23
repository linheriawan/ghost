//! Application event loop — multi-window orchestration owned by the app, not ghost_ui.

use std::time::Instant;

use ghost_ui::{CalloutApp, EventLoop, ExtraWindow, GhostApp, GhostEvent, GhostWindow, Widget, WidgetRenderer};
use tao::event::{ElementState, Event, MouseButton, WindowEvent};
use tao::event_loop::ControlFlow;

/// Run the application event loop with a main ghost window, a linked callout window,
/// and any number of extra windows (chat, log, controller, etc.).
pub fn run<A, C>(
    mut main_window: GhostWindow,
    mut callout_window: GhostWindow,
    callout_offset: [i32; 2],
    event_loop: EventLoop<()>,
    mut app: A,
    mut callout_app: C,
    mut extra_windows: Vec<Box<dyn ExtraWindow>>,
) where
    A: GhostApp + 'static,
    C: CalloutApp + 'static,
{
    let main_id = main_window.window().id();
    let callout_id = callout_window.window().id();

    let mut last_frame = Instant::now();
    let mut widget_renderer: Option<WidgetRenderer> = None;
    let mut main_gpu_ready = false;
    let mut callout_gpu_ready = false;

    let scale_factor = main_window.window().scale_factor();
    let scaled_callout_offset = [
        (callout_offset[0] as f64 * scale_factor) as i32,
        (callout_offset[1] as f64 * scale_factor) as i32,
    ];

    if let Some((x, y)) = main_window.outer_position() {
        callout_window.set_position(x + scaled_callout_offset[0], y + scaled_callout_offset[1]);
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            // ── Main window events ────────────────────────────────────────────
            Event::WindowEvent { window_id, event, .. } if window_id == main_id => {
                let window_height = main_window.window().inner_size().height as f32;

                match event {
                    WindowEvent::Focused(focused) => {
                        main_window.handle_focus(focused);
                        // handle_focus runs the skin hit test; also check layers and buttons.
                        if let Some(pos) = main_window.cursor_position() {
                            let (cx, cy) = (pos.x as f32, pos.y as f32);
                            let app_hit = app.hit_test(cx, cy)
                                || app.buttons().iter().any(|b| b.contains_point(cx, cy, window_height));
                            if app_hit {
                                main_window.apply_hit_test(true);
                            }
                        }
                        app.on_event(GhostEvent::FocusChanged(focused));
                        main_window.request_redraw();
                        if focused {
                            for extra in &extra_windows { extra.bring_to_front(); }
                        }
                    }

                    WindowEvent::CursorEntered { .. } => {
                        // Cursor entered: optimistically re-enable click detection so
                        // the next CursorMoved can do a precise hit test.
                        main_window.apply_hit_test(true);
                    }

                    WindowEvent::CursorMoved { position, .. } => {
                        main_window.handle_cursor_moved(position);
                        let (cx, cy) = (position.x as f32, position.y as f32);
                        // Include app layers and buttons in the hit test.
                        // Buttons and layers may sit over transparent skin pixels.
                        let app_hit = app.hit_test(cx, cy)
                            || app.buttons().iter().any(|b| b.contains_point(cx, cy, window_height));
                        if app_hit {
                            main_window.apply_hit_test(true);
                        }
                        for btn in app.buttons_mut() { btn.update_hover(cx, cy, window_height); }
                        for img in app.button_images_mut() { img.update_hover(cx, cy, window_height); }
                        main_window.request_redraw();
                    }

                    WindowEvent::CursorLeft { .. } => {
                        main_window.handle_cursor_left();
                        for btn in app.buttons_mut() { btn.update_hover(-1.0, -1.0, window_height); }
                        for img in app.button_images_mut() { img.update_hover(-1.0, -1.0, window_height); }
                    }

                    WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                        if let Some(pos) = main_window.cursor_position() {
                            let (cx, cy) = (pos.x as f32, pos.y as f32);
                            let pressed = app.buttons_mut().iter_mut().any(|b| b.handle_press(cx, cy, window_height))
                                || app.button_images_mut().iter_mut().any(|b| b.handle_press(cx, cy, window_height));
                            if !pressed && main_window.should_handle_click() && main_window.is_draggable() {
                                main_window.drag();
                            }
                            main_window.request_redraw();
                        }
                    }

                    WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                        if let Some(pos) = main_window.cursor_position() {
                            let (cx, cy) = (pos.x as f32, pos.y as f32);
                            let mut clicked: Vec<_> = app.buttons_mut().iter_mut()
                                .filter_map(|b| b.handle_release(cx, cy, window_height).then(|| b.id()))
                                .collect();
                            clicked.extend(app.button_images_mut().iter_mut()
                                .filter_map(|b| b.handle_release(cx, cy, window_height).then(|| b.id())));
                            for id in clicked { app.on_event(GhostEvent::ButtonClicked(id)); }
                            main_window.request_redraw();
                        }
                    }

                    WindowEvent::Resized(size) => {
                        main_window.handle_resize(size.width, size.height);
                        app.on_event(GhostEvent::Resized(size.width, size.height));
                    }

                    WindowEvent::Moved(pos) => {
                        callout_window.set_position(
                            pos.x + scaled_callout_offset[0],
                            pos.y + scaled_callout_offset[1],
                        );
                        app.on_event(GhostEvent::Moved(pos.x, pos.y));
                    }

                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    _ => {}
                }
            }

            // ── Callout window events ─────────────────────────────────────────
            Event::WindowEvent { window_id, event, .. } if window_id == callout_id => {
                if let WindowEvent::CloseRequested = event {
                    *control_flow = ControlFlow::Exit;
                }
            }

            // ── Extra windows (chat, log, controller, …) ──────────────────────
            Event::WindowEvent { window_id, event, .. } => {
                for extra in &mut extra_windows {
                    if extra.window_id() == window_id {
                        extra.on_event(&event);
                        extra.request_redraw();
                        break;
                    }
                }
            }

            // ── Per-frame update ──────────────────────────────────────────────
            Event::MainEventsCleared => {
                let now = Instant::now();
                let delta = now.duration_since(last_frame).as_secs_f32();
                let target_fps = app.current_skin().map(|_| 24.0).unwrap_or(10.0);
                last_frame = now;

                app.update(delta);
                app.on_event(GhostEvent::Update(delta));
                let callout_changed = callout_app.update(delta);
                for marquee in app.marquee_labels_mut() { marquee.update(delta); }
                for extra in &mut extra_windows { extra.update(delta); }

                if app.should_quit() { *control_flow = ControlFlow::Exit; return; }
                if app.current_skin().is_some() { main_window.request_redraw(); }
                if callout_changed { callout_window.request_redraw(); }
                for extra in &extra_windows {
                    if extra.is_visible() { extra.request_redraw(); }
                }

                *control_flow = ControlFlow::WaitUntil(
                    now + std::time::Duration::from_secs_f32(1.0 / target_fps),
                );
            }

            // ── Render: main window ───────────────────────────────────────────
            Event::RedrawRequested(window_id) if window_id == main_id => {
                if !main_gpu_ready {
                    if let Some(wr) = main_window.init_app_gpu(&mut app) {
                        widget_renderer = Some(wr);
                        main_gpu_ready = true;
                    }
                }
                let size = main_window.window().inner_size();
                let viewport = [size.width as f32, size.height as f32];
                if let Some(ref mut wr) = widget_renderer {
                    main_window.widget_prepare(wr, &mut app, viewport);
                }
                main_window.prepare_app(&mut app, viewport);
                let _ = main_window.render_with_widgets_and_app(widget_renderer.as_ref(), &mut app);
            }

            // ── Render: callout window ────────────────────────────────────────
            Event::RedrawRequested(window_id) if window_id == callout_id => {
                if !callout_gpu_ready {
                    callout_window.init_callout_gpu(&mut callout_app);
                    callout_gpu_ready = true;
                }
                let size = callout_window.window().inner_size();
                let viewport = [size.width as f32, size.height as f32];
                callout_window.prepare_callout(&mut callout_app, viewport);
                let _ = callout_window.render_callout(&callout_app);
            }

            // ── Render: extra windows ─────────────────────────────────────────
            Event::RedrawRequested(window_id) => {
                for extra in &mut extra_windows {
                    if extra.window_id() == window_id {
                        extra.render();
                        break;
                    }
                }
            }

            _ => {}
        }
    });
}
