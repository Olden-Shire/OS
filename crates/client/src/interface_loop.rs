// @ObfuscatedName("cz.ft") / "eg.fx" — Client.loopInterface /
// Client.loopLayer, plus the per-tick hook-queue drain and the IF3
// component drag loop (loopIf3Drag).
//
// This is the update half of the interface system: it walks the open
// interface tree once per game tick collecting cs2 hook events
// (clicks, hovers, timers, transmit notifications, key presses) into
// the three hook queues, tracks the v1 hover/tooltip/scrollbar state
// the draw pass reads, and maintains the IF3 drag state machine.
// Verbatim ports of Client.java:10933-11302 (loopLayer),
// 2514-2597 (the updateGame head that snapshots state, runs the
// pass, drains the queues and steps the drag), and 11372-11470
// (loopIf3Drag).

use crate::client::Client;
use crate::config::if_type::{self, IfType};
use crate::script_runner::{ComRef, HookReq};

// A walked component's identity: decoded components keep subId == -1
// (only cc_create assigns slots), so the ComRef shape is derivable.
pub fn com_ref_of(com: &IfType) -> ComRef {
    if com.sub_id >= 0 {
        ComRef::Cc { com: com.parent_id, sub: com.sub_id }
    } else {
        ComRef::Com(com.parent_id)
    }
}

// Java updateGame 2514-2597 — the per-tick interface update. Runs
// after the key buffer fills (onkey hooks read it) and before the
// ground-pick / mouseLoop tail of game_input.
pub fn interface_tick(c: &mut Client) {
    // Java 2514-2520: reset the per-tick hover/drag collectors. The
    // componentUpdated dirty-diff on over/tooltip change is dirty-rect
    // bookkeeping we don't depend on.
    c.over_com_id = -1;
    c.tooltip_com_id = -1;
    c.drop_com = ComRef::None;
    c.dragging = false;
    c.drag_parent_found = false;

    // Java 2529: loopInterface(toplevelinterface, 0, 0, 765, 503, 0, 0)
    if c.toplevelinterface != -1 {
        loop_interface(c, c.toplevelinterface, 0, 0, 765, 503, 0, 0);
    }
    c.transmit_num += 1;

    // Java 2533-2593: drain the queues — timer first, then mouse-stop,
    // then general; each pop validates that a cc component is still
    // attached (decoded components, subId < 0, always execute).
    loop {
        if let Some(req) = pop_valid(&mut c.hook_requests_timer) {
            crate::script_runner::execute_script(c, &req);
            continue;
        }
        if let Some(req) = pop_valid(&mut c.hook_requests_mouse_stop) {
            crate::script_runner::execute_script(c, &req);
            continue;
        }
        if let Some(req) = pop_valid(&mut c.hook_requests) {
            crate::script_runner::execute_script(c, &req);
            continue;
        }
        break;
    }

    // Java 2595-2597.
    if c.drag_com != ComRef::None {
        loop_if3_drag(c);
    }
}

// Java 2538-2550 — pop until a request passes the still-attached
// check. ccs (subId >= 0) must still occupy their parent slot;
// everything else executes unconditionally.
fn pop_valid(queue: &mut std::collections::VecDeque<HookReq>) -> Option<HookReq> {
    while let Some(req) = queue.pop_front() {
        match req.component {
            ComRef::Cc { .. } => {
                if req.component.resolve().is_some() {
                    return Some(req);
                }
                // Detached cc — drop the request and keep popping.
            }
            _ => return Some(req),
        }
    }
    None
}

// @ObfuscatedName("cz.ft(IIIIIIIS)V") — Client.loopInterface.
pub fn loop_interface(c: &mut Client, id: i32, x: i32, y: i32, w: i32, h: i32,
                      child_x: i32, child_y: i32) {
    let interfaces_slot = if_type::INTERFACES_SLOT
        .load(std::sync::atomic::Ordering::Relaxed);
    if !if_type::open_interface(id, interfaces_slot) {
        return;
    }
    let components: Vec<IfType> = {
        let s = if_type::STORE.lock().unwrap();
        match s.list.get(id as usize).and_then(|o| o.as_ref()) {
            Some(v) => v.iter().filter_map(|o| o.clone()).collect(),
            None => return,
        }
    };
    loop_layer(c, &components, -1, x, y, w, h, child_x, child_y);
}

// @ObfuscatedName("eg.fx([Leg;IIIIIIIB)V") — Client.loopLayer.
// Returns false when the pass aborted (Java's `return` inside the
// !v3 block when a drag or open menu suppresses hover handling).
#[allow(clippy::too_many_arguments)]
fn loop_layer(c: &mut Client, children: &[IfType], layer_id: i32,
              x: i32, y: i32, w: i32, h: i32,
              child_x: i32, child_y: i32) -> bool {
    let _g = crate::debug_depth::DepthGuard::enter("loop_layer", 400);
    let (mouse_x, mouse_y, mouse_button, click_button, click_x, click_y) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_x, m.mouse_y, m.mouse_button,
         m.mouse_click_button, m.mouse_click_x, m.mouse_click_y)
    };

    for com in children {
        let cref = com_ref_of(com);

        // Java 10945-10952 — skip filter: inert v3 leaves (no hook, no
        // server-active flags, not the drag layer) cost nothing.
        if com.v3 && com.type_ != 0 && !com.hashook
            && crate::client::get_active(c, com) == 0
            && c.drag_layer != cref
        {
            continue;
        }
        if com.layer_id != layer_id {
            continue;
        }
        if com.v3 && com.hide {
            continue;
        }

        let var10 = com.x + child_x;
        let var11 = com.y + child_y;
        let (var12, var13, var14, var15) = if com.type_ == 2 {
            (x, y, w, h)
        } else if com.type_ == 9 {
            let mut x1 = var10;
            let mut y1 = var11;
            let mut x2 = com.width + var10;
            let mut y2 = com.height + var11;
            if x2 < var10 { std::mem::swap(&mut x1, &mut x2); }
            if y2 < var11 { std::mem::swap(&mut y1, &mut y2); }
            x2 += 1;
            y2 += 1;
            (x1.max(x), y1.max(y), x2.min(w), y2.min(h))
        } else {
            (
                var10.max(x),
                var11.max(y),
                (com.width + var10).min(w),
                (com.height + var11).min(h),
            )
        };

        // Java 10995-10999 — the dragged component's live screen
        // anchor for the dead-zone test in loopIf3Drag.
        if c.drag_com != ComRef::None && c.drag_com == cref {
            c.dragging = true;
            c.drag_current_x = var10;
            c.drag_current_y = var11;
        }

        if com.v3 && (var12 >= var14 || var13 >= var15) {
            continue;
        }

        if com.client_code == crate::scene::CLIENT_CODE_VIEWPORT {
            // Java marks the viewport dirty — dirty-rect only.
            continue;
        }
        if com.client_code == crate::scene::CLIENT_CODE_MINIMAP {
            // Java drives minimapLoop from here; our game_input calls
            // minimap_loop directly with the click snapshot.
            continue;
        }

        if com.type_ == 0 {
            // Java 11011-11013 — hidden v1 layers skip unless hovered.
            if !com.v3 && com.hide
                && !(c.over_com_id != -1 && c.over_com_id == com.parent_id)
            {
                continue;
            }

            if !loop_layer(c, children, com.parent_id, var12, var13, var14, var15,
                           var10 - com.scroll_pos_x, var11 - com.scroll_pos_y) {
                return false;
            }
            if !com.subcomponents.is_empty() {
                let subs: Vec<IfType> =
                    com.subcomponents.iter().filter_map(|o| o.clone()).collect();
                if !loop_layer(c, &subs, com.parent_id, var12, var13, var14, var15,
                               var10 - com.scroll_pos_x, var11 - com.scroll_pos_y) {
                    return false;
                }
            }
            if let Some(sub) = c.subinterfaces.get(&com.parent_id).cloned() {
                if sub.id >= 0 {
                    loop_interface(c, sub.id, var12, var13, var14, var15, var10, var11);
                }
            }
        }

        if com.v3 {
            // Java 11026-11042 — hover / held / fresh-click predicates.
            let mut hovered = mouse_x >= var12 && mouse_y >= var13
                && mouse_x < var14 && mouse_y < var15;
            let mut held = mouse_button == 1 && hovered;
            let mut clicked = click_button == 1
                && click_x >= var12 && click_y >= var13
                && click_x < var14 && click_y < var15;

            if clicked {
                crate::client::drag_try_pickup(c, cref, click_x - var10, click_y - var11);
            }

            if c.drag_com != ComRef::None && c.drag_com != cref && hovered
                && crate::config::server_active::is_drag_target(
                    crate::client::get_active(c, com))
            {
                c.drop_com = cref;
            }

            if c.drag_layer == cref && c.drag_layer != ComRef::None {
                c.drag_parent_found = true;
                c.drag_parent_x = var10;
                c.drag_parent_y = var11;
            }

            if com.hashook {
                if hovered && c.mouse_wheel_rotation != 0
                    && com.hook_onscrollwheel.is_some()
                {
                    c.hook_requests.push_back(HookReq {
                        component: cref,
                        mouse_y: c.mouse_wheel_rotation,
                        onop: com.hook_onscrollwheel.clone().unwrap(),
                        ..Default::default()
                    });
                }

                // Java 11069-11073 — drags and the open menu suppress
                // the mouse-derived hooks (but not timers/transmits).
                if c.drag_com != ComRef::None || c.obj_drag_com != -1 || c.is_menu_open {
                    clicked = false;
                    held = false;
                    hovered = false;
                }

                // click/mouse trigger state machine — Java mutates the
                // component's fields mid-sequence, so track locally and
                // write back once.
                let mut click_trigger = com.click_trigger != 0;
                let mut mouse_trigger = com.mouse_trigger != 0;

                if !click_trigger && clicked {
                    click_trigger = true;
                    if let Some(hook) = com.hook_onclick.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref,
                            mouse_x: click_x - var10,
                            mouse_y: click_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if click_trigger && held {
                    if let Some(hook) = com.hook_onclickrepeat.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref,
                            mouse_x: mouse_x - var10,
                            mouse_y: mouse_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if click_trigger && !held {
                    click_trigger = false;
                    if let Some(hook) = com.hook_onrelease.clone() {
                        c.hook_requests_mouse_stop.push_back(HookReq {
                            component: cref,
                            mouse_x: mouse_x - var10,
                            mouse_y: mouse_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if held {
                    if let Some(hook) = com.hook_onhold.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref,
                            mouse_x: mouse_x - var10,
                            mouse_y: mouse_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if !mouse_trigger && hovered {
                    mouse_trigger = true;
                    if let Some(hook) = com.hook_onmouseover.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref,
                            mouse_x: mouse_x - var10,
                            mouse_y: mouse_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if mouse_trigger && hovered {
                    if let Some(hook) = com.hook_onmouserepeat.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref,
                            mouse_x: mouse_x - var10,
                            mouse_y: mouse_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if mouse_trigger && !hovered {
                    mouse_trigger = false;
                    if let Some(hook) = com.hook_onmouseleave.clone() {
                        c.hook_requests_mouse_stop.push_back(HookReq {
                            component: cref,
                            mouse_x: mouse_x - var10,
                            mouse_y: mouse_y - var11,
                            onop: hook,
                            ..Default::default()
                        });
                    }
                }

                if click_trigger != (com.click_trigger != 0)
                    || mouse_trigger != (com.mouse_trigger != 0)
                {
                    let (ct, mt) = (click_trigger as i32, mouse_trigger as i32);
                    cref.modify(|t| {
                        t.click_trigger = ct;
                        t.mouse_trigger = mt;
                    });
                }

                if let Some(hook) = com.hook_ontimer.clone() {
                    c.hook_requests_timer.push_back(HookReq {
                        component: cref,
                        onop: hook,
                        ..Default::default()
                    });
                }

                // Transmit hooks — Java 11161-11264. Each compares the
                // global counter to the component's stamp; the var/inv/
                // stat variants filter against the 32-deep change ring
                // when the component subscribed to specific ids.
                if let Some(hook) = com.hook_onvartransmit.clone() {
                    if c.var_transmit_num > com.var_transmit_num {
                        let fire = ring_hit(
                            com.on_var_transmit_list.as_deref(),
                            &c.var_transmit,
                            com.var_transmit_num, c.var_transmit_num);
                        if fire {
                            c.hook_requests.push_back(HookReq {
                                component: cref, onop: hook, ..Default::default()
                            });
                        }
                        let stamp = c.var_transmit_num;
                        cref.modify(|t| t.var_transmit_num = stamp);
                    }
                }

                if let Some(hook) = com.hook_oninvtransmit.clone() {
                    if c.inv_transmit_num > com.inv_transmit_num {
                        let fire = ring_hit(
                            com.on_inv_transmit_list.as_deref(),
                            &c.inv_transmit,
                            com.inv_transmit_num, c.inv_transmit_num);
                        if fire {
                            c.hook_requests.push_back(HookReq {
                                component: cref, onop: hook, ..Default::default()
                            });
                        }
                        let stamp = c.inv_transmit_num;
                        cref.modify(|t| t.inv_transmit_num = stamp);
                    }
                }

                if let Some(hook) = com.hook_onstattransmit.clone() {
                    if c.stat_transmit_num > com.stat_transmit_num {
                        let fire = ring_hit(
                            com.on_stat_transmit_list.as_deref(),
                            &c.stat_transmit,
                            com.stat_transmit_num, c.stat_transmit_num);
                        if fire {
                            c.hook_requests.push_back(HookReq {
                                component: cref, onop: hook, ..Default::default()
                            });
                        }
                        let stamp = c.stat_transmit_num;
                        cref.modify(|t| t.stat_transmit_num = stamp);
                    }
                }

                if c.chat_transmit_num > com.transmit_num {
                    if let Some(hook) = com.hook_onchattransmit.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref, onop: hook, ..Default::default()
                        });
                    }
                }
                if c.friend_transmit_num > com.transmit_num {
                    if let Some(hook) = com.hook_onfriendtransmit.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref, onop: hook, ..Default::default()
                        });
                    }
                }
                if c.clan_transmit_num > com.transmit_num {
                    if let Some(hook) = com.hook_onclantransmit.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref, onop: hook, ..Default::default()
                        });
                    }
                }
                if c.misc_transmit_num > com.transmit_num {
                    if let Some(hook) = com.hook_onmisctransmit.clone() {
                        c.hook_requests.push_back(HookReq {
                            component: cref, onop: hook, ..Default::default()
                        });
                    }
                }
                let stamp = c.transmit_num;
                cref.modify(|t| t.transmit_num = stamp);

                if let Some(hook) = com.hook_onkey.clone() {
                    for k in 0..c.keypresses as usize {
                        c.hook_requests.push_back(HookReq {
                            component: cref,
                            key_code: c.keypress_codes[k],
                            key_char: c.keypress_chars[k] as i32,
                            onop: hook.clone(),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        if !com.v3 {
            // Java 11280-11282 — a live drag or open menu suppresses
            // every remaining v1 hover/scroll consumer this tick.
            if c.drag_com != ComRef::None || c.obj_drag_com != -1 || c.is_menu_open {
                return false;
            }

            // Java 11284-11290 — overCom: components that request
            // hover colours, optionally redirecting into a sibling.
            if (com.over_layer_id >= 0 || com.colour_over != 0)
                && mouse_x >= var12 && mouse_y >= var13
                && mouse_x < var14 && mouse_y < var15
            {
                c.over_com_id = if com.over_layer_id >= 0 {
                    children.get(com.over_layer_id as usize)
                        .map_or(com.parent_id, |o| o.parent_id)
                } else {
                    com.parent_id
                };
            }

            // Java 11292-11294 — tooltip hover target.
            if com.type_ == 8
                && mouse_x >= var12 && mouse_y >= var13
                && mouse_x < var14 && mouse_y < var15
            {
                c.tooltip_com_id = com.parent_id;
            }

            // Java 11296-11298 — scrollbar input.
            if com.scroll_height > com.height {
                crate::client::do_scrollbar(
                    c, com, com.width + var10, var11,
                    com.height, com.scroll_height, mouse_x, mouse_y);
            }
        }
    }
    true
}

// The label383-style ring filter (Client.java:11162-11181): no
// subscription list (or a stamp gap wider than the 32-entry ring)
// always fires; otherwise fire only when one of the changed ids is in
// the component's list.
fn ring_hit(list: Option<&[i32]>, ring: &[i32; 32], from: i32, to: i32) -> bool {
    let Some(list) = list else { return true; };
    if to - from > 32 {
        return true;
    }
    for i in from..to {
        let changed = ring[(i & 0x1F) as usize];
        if list.contains(&changed) {
            return true;
        }
    }
    false
}

// @ObfuscatedName(— Client.loopIf3Drag). Verbatim port of
// Client.java:11372-11470: step the drag timer, clamp the dragged
// component to its drag layer, fire ondrag while alive, and on mouse
// release fire ondragcomplete + the IF_BUTTOND drop packet (or fall
// back to the menu click that started the drag).
pub fn loop_if3_drag(c: &mut Client) {
    c.drag_time += 1;

    let (mouse_x, mouse_y, mouse_button) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_x, m.mouse_y, m.mouse_button)
    };

    if c.dragging && c.drag_parent_found {
        let (Some(drag_com), Some(drag_layer)) =
            (c.drag_com.resolve(), c.drag_layer.resolve())
        else {
            c.drag_com = ComRef::None;
            return;
        };

        let mut drag_x = mouse_x - c.drag_pickup_x;
        let mut drag_y = mouse_y - c.drag_pickup_y;

        if drag_x < c.drag_parent_x {
            drag_x = c.drag_parent_x;
        }
        if drag_com.width + drag_x > c.drag_parent_x + drag_layer.width {
            drag_x = c.drag_parent_x + drag_layer.width - drag_com.width;
        }
        if drag_y < c.drag_parent_y {
            drag_y = c.drag_parent_y;
        }
        if drag_com.height + drag_y > c.drag_parent_y + drag_layer.height {
            drag_y = c.drag_parent_y + drag_layer.height - drag_com.height;
        }

        let dx = drag_x - c.drag_current_x;
        let dy = drag_y - c.drag_current_y;
        let dead = drag_com.drag_dead_zone;
        if c.drag_time > drag_com.drag_dead_time
            && (dx > dead || dx < -dead || dy > dead || dy < -dead)
        {
            c.drag_alive = true;
        }

        let hook_x = drag_layer.scroll_pos_x + (drag_x - c.drag_parent_x);
        let hook_y = drag_layer.scroll_pos_y + (drag_y - c.drag_parent_y);

        if c.drag_alive {
            if let Some(hook) = drag_com.hook_ondrag.clone() {
                let req = HookReq {
                    component: c.drag_com,
                    mouse_x: hook_x,
                    mouse_y: hook_y,
                    onop: hook,
                    ..Default::default()
                };
                crate::script_runner::execute_script(c, &req);
            }
        }

        if mouse_button == 0 {
            if c.drag_alive {
                if let Some(hook) = drag_com.hook_ondragcomplete.clone() {
                    let req = HookReq {
                        component: c.drag_com,
                        mouse_x: hook_x,
                        mouse_y: hook_y,
                        drop: c.drop_com,
                        onop: hook,
                        ..Default::default()
                    };
                    crate::script_runner::execute_script(c, &req);
                }

                if c.drop_com != ComRef::None {
                    // Java 11429-11458 — only transmit when the
                    // serverDraggable ancestor walk resolves (the
                    // server flagged this component draggable).
                    let levels = crate::config::server_active::server_draggable(
                        crate::client::get_active(c, &drag_com));
                    let mut confirmed = levels != 0;
                    if confirmed {
                        let mut cur = drag_com.clone();
                        for _ in 0..levels {
                            match if_type::get(cur.layer_id) {
                                Some(next) => cur = next,
                                None => {
                                    confirmed = false;
                                    break;
                                }
                            }
                        }
                    }

                    if confirmed {
                        let drag_sub = c.drag_com.sub_id();
                        let drag_parent = c.drag_com.parent_id();
                        let drop_parent = c.drop_com.parent_id();
                        let drop_sub = c.drop_com.sub_id();
                        if let (Some(isaac), Some(out)) =
                            (c.isaac_out.as_mut(), c.out_packet.as_mut())
                        {
                            // IF_BUTTOND
                            out.p1_enc(22, isaac);
                            out.p2_alt3(drag_sub);
                            out.p4_alt2(drop_parent);
                            out.p2_alt1(drop_sub);
                            out.p4_alt2(drag_parent);
                        }
                    }
                }
            } else if (c.one_mouse_button == 1
                || crate::client::is_add_friend_option(
                    crate::client::menu_action_at(c, c.menu_num_entries - 1)))
                && c.menu_num_entries > 2
            {
                let b12 = c.b12.clone();
                crate::client::open_menu(c, mouse_x, mouse_y, |s: &str| {
                    b12.as_ref().map_or(s.len() as i32 * 8, |f| f.base.string_wid(s))
                });
            } else if c.menu_num_entries > 0 {
                crate::client::do_action(c, c.menu_num_entries - 1);
            }

            c.drag_com = ComRef::None;
        }
    } else if c.drag_time > 1 {
        c.drag_com = ComRef::None;
    }
}
