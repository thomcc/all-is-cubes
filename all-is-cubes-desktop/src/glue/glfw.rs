// Copyright 2020-2022 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

use all_is_cubes::camera::Viewport;
use all_is_cubes::cgmath::Vector2;
use glfw::Glfw;

pub fn window_size_as_viewport(window: &glfw::Window) -> Viewport {
    Viewport {
        nominal_size: Vector2::from(window.get_size()).map(|s| s.into()),
        framebuffer_size: Vector2::from(window.get_framebuffer_size()).map(|s| s as u32),
    }
}

pub fn map_mouse_button(button: glfw::MouseButton) -> usize {
    use glfw::MouseButton::*;
    match button {
        Button1 => 0,
        Button2 => 1,
        Button3 => 2,
        Button4 => 3,
        Button5 => 4,
        Button6 => 5,
        Button7 => 6,
        Button8 => 7,
    }
}

pub fn map_key(key: glfw::Key) -> Option<all_is_cubes::apps::Key> {
    use all_is_cubes::apps::Key as A;
    use glfw::Key as G;
    Some(match key {
        G::Space => A::Character(' '),
        G::Apostrophe => A::Character('\''),
        G::Comma => A::Character(','),
        G::Minus => A::Character('-'),
        G::Period => A::Character('.'),
        G::Slash => A::Character('/'),
        G::Num0 => A::Character('0'),
        G::Num1 => A::Character('1'),
        G::Num2 => A::Character('2'),
        G::Num3 => A::Character('3'),
        G::Num4 => A::Character('4'),
        G::Num5 => A::Character('5'),
        G::Num6 => A::Character('6'),
        G::Num7 => A::Character('7'),
        G::Num8 => A::Character('8'),
        G::Num9 => A::Character('9'),
        G::Semicolon => A::Character(';'),
        G::Equal => A::Character('='),
        G::A => A::Character('a'),
        G::B => A::Character('b'),
        G::C => A::Character('c'),
        G::D => A::Character('d'),
        G::E => A::Character('e'),
        G::F => A::Character('f'),
        G::G => A::Character('g'),
        G::H => A::Character('h'),
        G::I => A::Character('i'),
        G::J => A::Character('j'),
        G::K => A::Character('k'),
        G::L => A::Character('l'),
        G::M => A::Character('m'),
        G::N => A::Character('n'),
        G::O => A::Character('o'),
        G::P => A::Character('p'),
        G::Q => A::Character('q'),
        G::R => A::Character('r'),
        G::S => A::Character('s'),
        G::T => A::Character('t'),
        G::U => A::Character('u'),
        G::V => A::Character('v'),
        G::W => A::Character('w'),
        G::X => A::Character('x'),
        G::Y => A::Character('y'),
        G::Z => A::Character('z'),
        G::LeftBracket => A::Character('['),
        G::Backslash => A::Character('\\'),
        G::RightBracket => A::Character(']'),
        G::GraveAccent => A::Character('`'),
        G::World1 => return None,
        G::World2 => return None,
        G::Escape => return None,       // TODO add this?
        G::Enter => A::Character('\r'), // TODO is this the mapping we want?
        G::Tab => A::Character('\t'),
        G::Backspace => A::Character('\u{8}'), // TODO is this the mapping we want?
        G::Insert => return None,
        G::Delete => return None,
        G::Right => A::Right,
        G::Left => A::Left,
        G::Down => A::Down,
        G::Up => A::Up,
        G::PageUp => return None,
        G::PageDown => return None,
        G::Home => return None,
        G::End => return None,
        G::CapsLock => return None,
        G::ScrollLock => return None,
        G::NumLock => return None,
        G::PrintScreen => return None,
        G::Pause => return None,
        G::F1 => return None,
        G::F2 => return None,
        G::F3 => return None,
        G::F4 => return None,
        G::F5 => return None,
        G::F6 => return None,
        G::F7 => return None,
        G::F8 => return None,
        G::F9 => return None,
        G::F10 => return None,
        G::F11 => return None,
        G::F12 => return None,
        G::F13 => return None,
        G::F14 => return None,
        G::F15 => return None,
        G::F16 => return None,
        G::F17 => return None,
        G::F18 => return None,
        G::F19 => return None,
        G::F20 => return None,
        G::F21 => return None,
        G::F22 => return None,
        G::F23 => return None,
        G::F24 => return None,
        G::F25 => return None,
        G::Kp0 => A::Character('0'),
        G::Kp1 => A::Character('1'),
        G::Kp2 => A::Character('2'),
        G::Kp3 => A::Character('3'),
        G::Kp4 => A::Character('4'),
        G::Kp5 => A::Character('5'),
        G::Kp6 => A::Character('6'),
        G::Kp7 => A::Character('7'),
        G::Kp8 => A::Character('8'),
        G::Kp9 => A::Character('9'),
        G::KpDecimal => A::Character('.'),
        G::KpDivide => A::Character('/'),
        G::KpMultiply => A::Character('*'),
        G::KpSubtract => A::Character('-'),
        G::KpAdd => A::Character('+'),
        G::KpEnter => A::Character('\r'), // TODO is this the mapping we want?
        G::KpEqual => A::Character('='),
        G::LeftShift => return None,
        G::LeftControl => return None,
        G::LeftAlt => return None,
        G::LeftSuper => return None,
        G::RightShift => return None,
        G::RightControl => return None,
        G::RightAlt => return None,
        G::RightSuper => return None,
        G::Menu => return None,
        G::Unknown => return None,
    })
}

pub fn get_primary_workarea_size(glfw: &mut Glfw) -> Option<Vector2<u32>> {
    glfw.with_primary_monitor(|_glfw, opt_primary_monitor| {
        opt_primary_monitor.map(|m| {
            let (_, _, width, height) = m.get_workarea();
            Vector2::new(width as u32, height as u32)
        })
    })
}
