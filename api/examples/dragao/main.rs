//! Dragão: `/dragao [cor]` builds a little dragon of blocks in front of the
//! player (along +x), with wings, horns, a tail and fire at its mouth.
//! `cor` is the body's concrete colour (`green`, `red`, `black`...).

use horizon_plugin_api::prelude::*;

/// What a block of the dragon is.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Body,
    Belly,
    Wing,
    Horn,
    Eye,
    Spike,
}

/// The dragon's blocks: (x along the body, y up, z across, part).
fn dragon() -> Vec<(i32, i32, i32, Part)> {
    use Part::*;
    let mut b = Vec::new();
    // Body: 6 long, 2 tall, 3 wide, light underneath.
    for x in 0..6 {
        for z in -1..=1 {
            b.push((x, 1, z, Belly));
            b.push((x, 2, z, Body));
        }
    }
    // Legs.
    for (x, z) in [(1, -1), (1, 1), (4, -1), (4, 1)] {
        b.push((x, 0, z, Body));
    }
    // Spikes along the back.
    for x in [1, 3, 5] {
        b.push((x, 3, 0, Spike));
    }
    // Neck and head.
    b.push((6, 3, 0, Body));
    b.push((7, 4, 0, Body));
    for x in 8..=10 {
        for y in 4..=5 {
            for z in -1..=1 {
                b.push((x, y, z, Body));
            }
        }
    }
    b.push((11, 4, 0, Body));
    b.push((10, 5, -1, Eye));
    b.push((10, 5, 1, Eye));
    b.push((8, 6, -1, Horn));
    b.push((8, 6, 1, Horn));
    b.push((7, 7, -1, Horn));
    b.push((7, 7, 1, Horn));
    // Tail, rising to a spiked tip.
    for (x, y) in [(-1, 2), (-2, 2), (-3, 3), (-4, 3), (-5, 4)] {
        b.push((x, y, 0, Body));
    }
    b.push((-6, 5, 0, Spike));
    // Wings, wider at the body, rising outwards on both sides.
    for (d, from, to) in [(2, 1, 5), (3, 1, 5), (4, 2, 4), (5, 2, 4), (6, 3, 3)] {
        for x in from..=to {
            for side in [-1, 1] {
                b.push((x, 2 + d / 2, side * d, Wing));
            }
        }
    }
    b
}

fn block(part: Part, color: &str) -> String {
    match part {
        Part::Body => format!("{color}_concrete"),
        Part::Belly => "lime_concrete".to_owned(),
        Part::Wing => "red_concrete".to_owned(),
        Part::Horn => "bone_block".to_owned(),
        Part::Eye => "yellow_concrete".to_owned(),
        Part::Spike => "red_concrete".to_owned(),
    }
}

const COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray",
    "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
];

#[derive(Default)]
struct Dragao {
    built: u32,
}

impl Plugin for Dragao {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.command(
            CommandSpec::new("dragao")
                .description("Builds a little dragon in front of you")
                .usage("/<command> [cor]")
                .alias("dragon")
                .permission("dragao.build"),
            |plugin, ctx, sender, _, args| {
                let Some(p) = sender.player() else {
                    sender.send_message("Only a player has somewhere to build.");
                    return true;
                };
                let color = args.first().map_or("green", String::as_str).to_ascii_lowercase();
                if !COLORS.contains(&color.as_str()) {
                    p.send_message(&format!("§cNo such colour. Pick one of: {}", COLORS.join(", ")));
                    return true;
                }
                // Three blocks in front, standing where the player stands.
                let base = p.location.block().offset(3, 0, 0);
                let world = p.world();
                let blocks = dragon();
                for &(x, y, z, part) in &blocks {
                    world.set_block(base.x + x, base.y + y, base.z + z, &block(part, &color));
                }
                // Fire at its mouth, six puffs half a second apart.
                let mouth = base.offset(12, 4, 0).location();
                let mut puffs = 0;
                ctx.run_timer(2, 10, move |_, ctx| {
                    World::new(&mouth.world).spawn_particle("flame", &mouth, 20, 0.2, 0.05);
                    puffs += 1;
                    if puffs == 6
                        && let Some(id) = ctx.current_task()
                    {
                        ctx.cancel_task(id);
                    }
                });
                plugin.built += 1;
                p.send_message(&format!(
                    "§aRawr! §7Dragon #{} ({} blocks) at {} {} {}.",
                    plugin.built,
                    blocks.len(),
                    base.x,
                    base.y,
                    base.z
                ));
                true
            },
        );
    }
}

horizon_plugin!(
    Dragao::default(),
    PluginDescription::new("Dragao", "1.0.0")
        .description("A little dragon of blocks")
        .author("Horizon")
        .permission(Permission::new("dragao.build").default(PermissionDefault::True))
);
