use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::events::{EventReceiver, ServerEvent};

/// Bevy Resource wrapping the mpsc receiver in a Mutex (Receiver is not Sync).
#[derive(Resource)]
pub struct ServerEventReceiver(pub Mutex<EventReceiver>);

/// Tracks the visual state of all nodes.
#[derive(Resource, Default)]
pub struct VisualNodeGraph {
    pub nodes: HashMap<String, Entity>,
    pub node_positions: HashMap<String, Vec2>,
    pub node_count_changed: bool,
}

/// Marker component for node circle entities.
#[derive(Component)]
pub struct NodeCircle {
    pub name: String,
}

/// Marker component for node label text.
#[derive(Component)]
pub struct NodeLabel;

/// Component for message animation entities.
#[derive(Component)]
pub struct MessageDot {
    pub from_pos: Vec2,
    pub to_pos: Vec2,
    pub lifetime: Timer,
}

pub struct StarsVisualizationPlugin;

impl Plugin for StarsVisualizationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VisualNodeGraph>().add_systems(
            Update,
            (
                poll_server_events,
                update_node_layout,
                animate_messages,
                draw_connections,
            ),
        );
    }
}

/// Drain the mpsc channel each frame and apply events.
fn poll_server_events(
    receiver: Res<ServerEventReceiver>,
    mut graph: ResMut<VisualNodeGraph>,
    mut commands: Commands,
) {
    let rx = receiver.0.lock().unwrap();
    while let Ok(event) = rx.try_recv() {
        match event {
            ServerEvent::NodeConnected { name } => {
                if !graph.nodes.contains_key(&name) {
                    let entity = commands
                        .spawn((
                            Sprite::from_color(
                                Color::srgb(0.2, 0.7, 1.0),
                                Vec2::new(40.0, 40.0),
                            ),
                            Transform::from_translation(Vec3::ZERO),
                            NodeCircle {
                                name: name.clone(),
                            },
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                Text2d::new(name.clone()),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                                Transform::from_translation(Vec3::new(0.0, -30.0, 1.0)),
                                NodeLabel,
                            ));
                        })
                        .id();
                    graph.nodes.insert(name, entity);
                    graph.node_count_changed = true;
                }
            }
            ServerEvent::NodeDisconnected { name } => {
                if let Some(entity) = graph.nodes.remove(&name) {
                    commands.entity(entity).despawn();
                }
                graph.node_positions.remove(&name);
                graph.node_count_changed = true;
            }
            ServerEvent::MessageRouted { from, to } => {
                let from_pos = graph
                    .node_positions
                    .get(&from)
                    .copied()
                    .unwrap_or(Vec2::ZERO);
                let to_pos = graph
                    .node_positions
                    .get(&to)
                    .copied()
                    .unwrap_or(Vec2::ZERO);

                commands.spawn((
                    Sprite::from_color(Color::srgb(1.0, 1.0, 0.3), Vec2::new(10.0, 10.0)),
                    Transform::from_translation(from_pos.extend(2.0)),
                    MessageDot {
                        from_pos,
                        to_pos,
                        lifetime: Timer::from_seconds(0.5, TimerMode::Once),
                    },
                ));
            }
        }
    }
}

/// Recompute node positions in a circle when node count changes, and lerp towards targets.
fn update_node_layout(
    mut graph: ResMut<VisualNodeGraph>,
    mut query: Query<(&NodeCircle, &mut Transform)>,
    windows: Query<&Window>,
) {
    if graph.node_count_changed {
        let node_count = graph.nodes.len();
        if node_count > 0 {
            if let Ok(window) = windows.single() {
                let radius = (window.width().min(window.height()) * 0.35).max(100.0);

                let mut new_positions = HashMap::new();
                for (i, name) in graph.nodes.keys().enumerate() {
                    let angle = (i as f32 / node_count as f32) * std::f32::consts::TAU;
                    let pos = Vec2::new(angle.cos(), angle.sin()) * radius;
                    new_positions.insert(name.clone(), pos);
                }
                graph.node_positions = new_positions;
            }
        }
        graph.node_count_changed = false;
    }

    for (node_circle, mut transform) in &mut query {
        if let Some(target) = graph.node_positions.get(&node_circle.name) {
            let current = transform.translation.truncate();
            let smoothed = current.lerp(*target, 0.1);
            transform.translation = smoothed.extend(0.0);
        }
    }
}

/// Animate message dots from source to target, despawn when done.
fn animate_messages(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut MessageDot, &mut Transform)>,
) {
    for (entity, mut msg, mut transform) in &mut query {
        msg.lifetime.tick(time.delta());
        let progress = msg.lifetime.fraction();
        let pos = msg.from_pos.lerp(msg.to_pos, progress);
        transform.translation = pos.extend(2.0);

        if msg.lifetime.fraction() >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Draw lines between all nodes using gizmos.
fn draw_connections(mut gizmos: Gizmos, graph: Res<VisualNodeGraph>) {
    let positions: Vec<Vec2> = graph.node_positions.values().copied().collect();
    let node_count = positions.len();
    if node_count < 2 {
        return;
    }

    let center = Vec2::ZERO;
    for pos in &positions {
        gizmos.line_2d(*pos, center, Color::srgba(0.3, 0.5, 0.8, 0.3));
    }
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

pub fn run_visualization(receiver: EventReceiver) {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "STARS Server - Node Visualization".to_string(),
                resolution: (1024u32, 768u32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ServerEventReceiver(Mutex::new(receiver)))
        .add_plugins(StarsVisualizationPlugin)
        .add_systems(Startup, setup_camera)
        .run();
}
