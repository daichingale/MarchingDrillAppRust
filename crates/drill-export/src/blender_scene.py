# DrillForge 3D scene. Open in Blender, or render headless:
#   blender -b -P drill_scene.py -- --render out.mp4
# Optional: --data drill_scene.json --stills DIR --night-stills DIR
#           --seconds 16 --samples 8 --engine eevee|cycles
#           --camera press_box|end_zone|orbit|follow
#           --width 1280 --height 720 --fps 24
#           --heroes DIR --heroes-only

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile

import bpy
import bmesh
from mathutils import Vector


def args_after_double_dash():
    if "--" in sys.argv:
        return sys.argv[sys.argv.index("--") + 1 :]
    return []


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", default="")
    parser.add_argument("--render", default="")
    parser.add_argument("--stills", default="")
    parser.add_argument("--night-stills", default="")
    parser.add_argument("--seconds", type=float, default=0.0)
    parser.add_argument("--samples", type=int, default=0)
    parser.add_argument("--engine", default="")
    parser.add_argument("--camera", default="")
    parser.add_argument("--width", type=int, default=0)
    parser.add_argument("--height", type=int, default=0)
    parser.add_argument("--fps", type=int, default=0)
    parser.add_argument("--heroes", default="")
    parser.add_argument("--heroes-only", action="store_true")
    return parser.parse_args(args_after_double_dash())


def script_dir():
    return os.path.dirname(os.path.abspath(__file__))


def load_scene(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def clear_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def link(obj):
    bpy.context.scene.collection.objects.link(obj)
    return obj


def mesh_object(name, mesh, location=(0.0, 0.0, 0.0)):
    obj = bpy.data.objects.new(name, mesh)
    obj.location = location
    return link(obj)


def shade(color, roughness=0.55, name="drill"):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    node = mat.node_tree.nodes.get("Principled BSDF")
    if node:
        node.inputs["Base Color"].default_value = (
            color[0] / 255.0,
            color[1] / 255.0,
            color[2] / 255.0,
            1.0,
        )
        if "Roughness" in node.inputs:
            node.inputs["Roughness"].default_value = roughness
    return mat


def striped_turf():
    mat = shade((48, 132, 58), 0.9, "turf")
    tree = mat.node_tree
    nodes = tree.nodes
    links = tree.links
    bsdf = nodes.get("Principled BSDF")
    if bsdf is None:
        return mat
    coord = nodes.new("ShaderNodeTexCoord")
    mapping = nodes.new("ShaderNodeMapping")
    mapping.inputs["Scale"].default_value = (0.42, 0.02, 1.0)
    wave = nodes.new("ShaderNodeTexWave")
    wave.wave_type = "BANDS"
    wave.bands_direction = "X"
    wave.inputs["Scale"].default_value = 1.0
    ramp = nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].color = (0.07, 0.30, 0.11, 1.0)
    ramp.color_ramp.elements[1].color = (0.24, 0.58, 0.24, 1.0)
    links.new(coord.outputs["Object"], mapping.inputs["Vector"])
    links.new(mapping.outputs["Vector"], wave.inputs["Vector"])
    links.new(wave.outputs["Fac"], ramp.inputs["Fac"])
    links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    return mat


def sky(night):
    world = bpy.data.worlds.new("sky")
    bpy.context.scene.world = world
    world.use_nodes = True
    tree = world.node_tree
    nodes = tree.nodes
    links = tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputWorld")
    bg = nodes.new("ShaderNodeBackground")
    coord = nodes.new("ShaderNodeTexCoord")
    mapping = nodes.new("ShaderNodeMapping")
    # Generated X is not "up". Turn the gradient so it runs from horizon to zenith.
    mapping.inputs["Rotation"].default_value = (0.0, math.radians(90), 0.0)
    grad = nodes.new("ShaderNodeTexGradient")
    grad.gradient_type = "LINEAR"
    ramp = nodes.new("ShaderNodeValToRGB")
    if night:
        ramp.color_ramp.elements[0].color = (0.02, 0.03, 0.07, 1.0)
        ramp.color_ramp.elements[1].color = (0.0, 0.0, 0.015, 1.0)
        bg.inputs["Strength"].default_value = 0.22
    else:
        ramp.color_ramp.elements[0].color = (0.16, 0.32, 0.62, 1.0)
        ramp.color_ramp.elements[1].color = (0.45, 0.58, 0.74, 1.0)
        bg.inputs["Strength"].default_value = 0.32
    links.new(coord.outputs["Generated"], mapping.inputs["Vector"])
    links.new(mapping.outputs["Vector"], grad.inputs["Vector"])
    links.new(grad.outputs["Fac"], ramp.inputs["Fac"])
    links.new(ramp.outputs["Color"], bg.inputs["Color"])
    links.new(bg.outputs["Background"], out.inputs["Surface"])


def to_world(field, x, y, z):
    return (
        float(x) - float(field["width"]) * 0.5,
        float(y) - float(field["height"]) * 0.5,
        float(z),
    )


def box_mesh(sx, sy, sz):
    mesh = bpy.data.meshes.new("box")
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for vert in bm.verts:
        vert.co.x *= sx
        vert.co.y *= sy
        vert.co.z *= sz
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def cylinder_mesh(radius, depth, vertices=8, origin="center"):
    mesh = bpy.data.meshes.new("cyl")
    bm = bmesh.new()
    bmesh.ops.create_cone(
        bm,
        cap_ends=True,
        segments=vertices,
        radius1=radius,
        radius2=radius,
        depth=depth,
    )
    if origin == "top":
        for vert in bm.verts:
            vert.co.z -= depth * 0.5
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def cone_mesh(radius, depth):
    mesh = bpy.data.meshes.new("cone")
    bm = bmesh.new()
    bmesh.ops.create_cone(
        bm,
        cap_ends=True,
        segments=8,
        radius1=radius,
        radius2=radius * 0.15,
        depth=depth,
    )
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def uv_sphere(radius, segments=8):
    mesh = bpy.data.meshes.new("sph")
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segments, v_segments=max(4, segments // 2), radius=radius)
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def paint(obj, material):
    data = obj.data
    if data is None or not hasattr(data, "materials"):
        return
    # Shared meshes stay uncolored. Each object carries its own slot so a
    # trumpet and a drum can share a torso mesh without sharing a uniform.
    if len(data.materials) == 0:
        data.materials.append(None)
    if not obj.material_slots:
        return
    slot = obj.material_slots[0]
    slot.link = "OBJECT"
    slot.material = material


def add_line(name, field, x, y, sx, sy, material, z=0.03):
    mesh = box_mesh(sx, sy, 0.02)
    obj = mesh_object(name, mesh, to_world(field, x, y, z))
    paint(obj, material)
    return obj


def yard_label(field, x, text, material, y=None):
    curve = bpy.data.curves.new("yard", "FONT")
    curve.body = text
    curve.size = 1.5
    curve.align_x = "CENTER"
    curve.align_y = "CENTER"
    curve.extrude = 0.015
    curve.resolution_u = 6
    font = ui_font()
    if font is not None:
        curve.font = font
    height = float(field["height"])
    # Sit on the near track, off the performing surface. No upright board.
    near = -1.6 if y is None else float(y)
    obj = bpy.data.objects.new(f"num-{text}-{x:.0f}", curve)
    obj.location = to_world(field, x, near, 0.08)
    obj.rotation_euler = (math.radians(72), 0.0, 0.0)
    link(obj)
    paint(obj, material)
    return obj


def build_field(scene):
    field = scene["field"]
    width = float(field["width"])
    height = float(field["height"])
    kind = field.get("kind") or "football"
    white = shade((236, 236, 228), 0.35, "white-line")
    gold = shade((250, 214, 92), 0.4, "frame")
    track = shade((176, 74, 58), 0.7, "track")
    concrete = shade((168, 166, 160), 0.85, "concrete")
    # Football, pitch, and contest floor get mowing stripes. Indoor stays wood.
    turf = striped_turf() if kind != "indoor" else shade((214, 176, 120), 0.75, "floor")
    # Surrounding ground, then a track or gym border, then the playing surface.
    ground = mesh_object("horizon", box_mesh(width + 80.0, height + 60.0, 0.05), (0.0, 0.0, -0.08))
    paint(ground, shade((58, 122, 62), 0.9, "horizon"))
    border = mesh_object(
        "border",
        box_mesh(width + 16.0, height + 14.0, 0.04),
        (0.0, 0.0, -0.02),
    )
    paint(border, track)
    lane = mesh_object(
        "lane",
        box_mesh(width + 9.0, height + 7.0, 0.025),
        (0.0, 0.0, -0.005),
    )
    paint(lane, shade((196, 92, 70), 0.65, "lane"))
    playing = mesh_object("field", box_mesh(width, height, 0.04), (0.0, 0.0, 0.0))
    paint(playing, turf)
    # White lane lines on the clay, just outside the playing surface.
    add_line("lane-s", field, width * 0.5, -1.2, width + 2.4, 0.14, white, 0.02)
    add_line("lane-n", field, width * 0.5, height + 1.2, width + 2.4, 0.14, white, 0.02)
    add_line("lane-w", field, -1.2, height * 0.5, 0.14, height + 2.4, white, 0.02)
    add_line("lane-e", field, width + 1.2, height * 0.5, 0.14, height + 2.4, white, 0.02)
    if kind == "football":
        # End zones sit past the goal lines.
        for name, x in (("near-end", -6.0), ("far-end", width + 6.0)):
            zone = mesh_object(name, box_mesh(10.0, height, 0.03), to_world(field, x, height * 0.5, 0.01))
            paint(zone, shade((30, 110, 52), 0.85, "endzone"))
        add_line("sideline-s", field, width * 0.5, 0.15, width, 0.18, white)
        add_line("sideline-n", field, width * 0.5, height - 0.15, width, 0.18, white)
        add_line("goal-w", field, 0.15, height * 0.5, 0.22, height, white)
        add_line("goal-e", field, width - 0.15, height * 0.5, 0.22, height, white)
        for x in field.get("major_x", []):
            add_line(f"yard-{x:.1f}", field, x, height * 0.5, 0.08, height, white)
            yards = min(float(x), width - float(x))
            number = int(round(yards))
            if number >= 10 and number % 10 == 0 and number <= 50:
                yard_label(field, x, str(number), white, height * 0.18)
        dash = box_mesh(0.12, 0.55, 0.02)
        for y in field.get("hashes", []):
            step = 1.0 if field.get("unit") == "yd" else 5.0
            x = 0.0
            while x <= width:
                mark = mesh_object(f"hash-{x:.0f}-{y:.0f}", dash, to_world(field, x, y, 0.035))
                paint(mark, white)
                x += step
    elif kind == "pitch":
        add_line("touch-s", field, width * 0.5, 0.2, width, 0.16, white)
        add_line("touch-n", field, width * 0.5, height - 0.2, width, 0.16, white)
        add_line("goal-w", field, 0.2, height * 0.5, 0.16, height, white)
        add_line("goal-e", field, width - 0.2, height * 0.5, 0.16, height, white)
        add_line("halfway", field, width * 0.5, height * 0.5, 0.16, height, white)
        circle = mesh_object("center", cylinder_mesh(9.15, 0.02, 24), to_world(field, width * 0.5, height * 0.5, 0.03))
        paint(circle, white)
    else:
        # Chalk the boundary so the track reads as a sideline, then the contest squares.
        add_line("sideline-s", field, width * 0.5, 0.12, width, 0.14, white)
        add_line("sideline-n", field, width * 0.5, height - 0.12, width, 0.14, white)
        add_line("sideline-w", field, 0.12, height * 0.5, 0.14, height, white)
        add_line("sideline-e", field, width - 0.12, height * 0.5, 0.14, height, white)
        # Indoor / school floor: contest squares and meter ticks, no yard hashes.
        for index, bounds in enumerate(field.get("frames", [])):
            min_x, min_y, max_x, max_y = bounds
            cx = (min_x + max_x) * 0.5
            cy = (min_y + max_y) * 0.5
            span_x = max(0.2, max_x - min_x)
            span_y = max(0.2, max_y - min_y)
            for edge, center, scale in (
                ("n", (cx, max_y), (span_x, 0.12)),
                ("s", (cx, min_y), (span_x, 0.12)),
                ("w", (min_x, cy), (0.12, span_y)),
                ("e", (max_x, cy), (0.12, span_y)),
            ):
                add_line(f"frame-{index}-{edge}", field, center[0], center[1], scale[0], scale[1], gold if index == 0 else white)
        grid_ink = shade((244, 246, 236), 0.4, "grid")
        for x in field.get("major_x", []):
            add_line(f"major-{x:.1f}", field, x, height * 0.5, 0.07, height * 0.92, grid_ink)
        # The major grid is snapped to the step, so it is not on the tens.
        # Paint 10, 20, 30… where the press box can read them.
        for meters in range(10, int(width), 10):
            add_line(
                f"yardline-{meters}",
                field,
                float(meters),
                height * 0.5,
                0.16,
                height * 0.88,
                grid_ink,
                0.045,
            )
            yard_label(field, float(meters), str(meters), shade((236, 236, 228), 0.4, "yard-ink"), -1.6)
    build_stands(field, kind)


def build_stands(field, kind):
    width = float(field["width"])
    height = float(field["height"])
    span = width * 0.7
    riser_mat = shade((58, 62, 70), 0.75, "riser")
    rail_mat = shade((214, 216, 210), 0.35, "rail")
    glass = shade((150, 186, 214), 0.15, "glass")
    frame = shade((48, 52, 58), 0.5, "booth")
    # Two banks of empty bleachers, a center aisle, and a seat back on every row.
    # Alternating seat colors and a gap between rows read as seats from the press box.
    bank_span = span * 0.46
    seat_red = shade((176, 64, 58), 0.6, "seat-red")
    seat_blue = shade((52, 86, 148), 0.6, "seat-blue")
    back_mat = shade((214, 214, 208), 0.45, "seat-back")
    aisle_mat = shade((168, 164, 156), 0.8, "aisle")
    for side, y_edge, outward in (("south", -6.4, -1.0), ("north", height + 6.4, 1.0)):
        fascia = mesh_object(
            f"fascia-{side}",
            box_mesh(span, 0.18, 1.15),
            to_world(field, width * 0.5, y_edge + outward * 0.1, 0.58),
        )
        paint(fascia, frame)
        for row in range(9):
            depth = 0.78
            rise = 0.46
            pitch = 1.22
            y = y_edge + outward * (0.55 + row * pitch)
            z = 0.28 + row * rise
            seat_color = seat_red if row % 2 == 0 else seat_blue
            for bank, x_off in (("w", -span * 0.26), ("e", span * 0.26)):
                block = mesh_object(
                    f"stand-{side}-{bank}-{row}",
                    box_mesh(bank_span, depth, rise * 0.92),
                    to_world(field, width * 0.5 + x_off, y, z),
                )
                paint(block, riser_mat)
                pad = mesh_object(
                    f"seat-{side}-{bank}-{row}",
                    box_mesh(bank_span * 0.94, 0.52, 0.09),
                    to_world(field, width * 0.5 + x_off, y - outward * 0.06, z + rise * 0.52),
                )
                paint(pad, seat_color)
                back = mesh_object(
                    f"back-{side}-{bank}-{row}",
                    box_mesh(bank_span * 0.94, 0.07, 0.42),
                    to_world(field, width * 0.5 + x_off, y - outward * 0.34, z + rise * 0.78),
                )
                paint(back, back_mat)
            tread = mesh_object(
                f"aisle-{side}-{row}",
                box_mesh(span * 0.07, 0.7, 0.08),
                to_world(field, width * 0.5, y, z + rise * 0.5),
            )
            paint(tread, aisle_mat)
        rail_y = y_edge + outward * 0.15
        rail = mesh_object(
            f"rail-{side}",
            box_mesh(span, 0.06, 0.08),
            to_world(field, width * 0.5, rail_y, 1.05),
        )
        paint(rail, rail_mat)
        posts = max(4, int(span / 8.0))
        for index in range(posts):
            x = width * 0.5 - span * 0.5 + (index + 0.5) * span / posts
            post = mesh_object(
                f"post-{side}-{index}",
                box_mesh(0.06, 0.06, 1.05),
                to_world(field, x, rail_y, 0.55),
            )
            paint(post, rail_mat)
    # Press box: deck, mullions, glass, roof, and a front rail. Not one slab.
    deck_y = -8.6
    deck = mesh_object("press-deck", box_mesh(12.0, 4.0, 0.35), to_world(field, width * 0.5, deck_y, 3.5))
    paint(deck, frame)
    roof = mesh_object("press-roof", box_mesh(13.0, 4.6, 0.22), to_world(field, width * 0.5, deck_y, 6.15))
    paint(roof, frame)
    glass_pane = mesh_object(
        "press-box",
        box_mesh(10.4, 0.12, 2.2),
        to_world(field, width * 0.5, deck_y - 1.7, 4.85),
    )
    paint(glass_pane, glass)
    for index, x_off in enumerate((-4.6, -2.3, 0.0, 2.3, 4.6)):
        mullion = mesh_object(
            f"press-mullion-{index}",
            box_mesh(0.12, 0.16, 2.3),
            to_world(field, width * 0.5 + x_off, deck_y - 1.7, 4.85),
        )
        paint(mullion, frame)
    booth_rail = mesh_object(
        "press-rail",
        box_mesh(11.2, 0.06, 0.08),
        to_world(field, width * 0.5, deck_y - 1.95, 4.05),
    )
    paint(booth_rail, rail_mat)
    if kind != "football":
        return
    add_line("rail", field, width * 0.5, -1.2, width, 0.08, rail_mat, 0.4)


def parent(child, parent_obj):
    child.parent = parent_obj
    child.matrix_parent_inverse = parent_obj.matrix_world.inverted()


def build_templates():
    return {
        "torso": box_mesh(0.34, 0.18, 0.55),
        "head": uv_sphere(0.11, 8),
        "shako": cylinder_mesh(0.12, 0.16, 8),
        "brim": cylinder_mesh(0.16, 0.02, 8),
        "leg": cylinder_mesh(0.055, 0.72, 6, origin="top"),
        "bell": cone_mesh(0.09, 0.28),
        "wood": cylinder_mesh(0.025, 0.42, 6),
        "drum": cylinder_mesh(0.16, 0.12, 8),
        "mallet": cylinder_mesh(0.012, 0.36, 5),
        "pole": cylinder_mesh(0.015, 1.15, 5),
        "flag": box_mesh(0.42, 0.02, 0.55),
        "rifle": box_mesh(0.04, 0.03, 0.85),
        "shadow": cylinder_mesh(0.32, 0.015, 8),
    }


def darker(color):
    return tuple(max(0, int(channel * 0.55)) for channel in color)


def lighter(color):
    return tuple(min(255, int(channel * 0.45 + 140)) for channel in color)


def add_prop(root, prop, templates, color, height):
    gold = shade((212, 170, 64), 0.35, "brass")
    black = shade((28, 28, 32), 0.4, "black-prop")
    cloth = shade(lighter(color), 0.6, "flag-cloth")
    if prop == "brass":
        bell = mesh_object("bell", templates["bell"], (0.0, -0.16, height * 0.55))
        bell.rotation_euler = (math.radians(80), 0.0, 0.0)
        paint(bell, gold)
        parent(bell, root)
    elif prop == "woodwind":
        tube = mesh_object("wood", templates["wood"], (0.08, -0.12, height * 0.5))
        tube.rotation_euler = (math.radians(70), 0.0, 0.2)
        paint(tube, black)
        parent(tube, root)
    elif prop == "drum":
        drum = mesh_object("drum", templates["drum"], (0.0, -0.02, height * 0.42))
        drum.rotation_euler = (math.radians(90), 0.0, 0.0)
        paint(drum, shade((240, 240, 236), 0.3, "head"))
        parent(drum, root)
    elif prop == "mallet":
        for side in (-0.08, 0.08):
            stick = mesh_object("mallet", templates["mallet"], (side, -0.1, height * 0.7))
            stick.rotation_euler = (math.radians(40), 0.0, side)
            paint(stick, black)
            parent(stick, root)
    elif prop == "flag":
        pole = mesh_object("pole", templates["pole"], (0.16, -0.05, height * 0.85))
        paint(pole, shade((230, 230, 226), 0.3, "pole"))
        parent(pole, root)
        flag = mesh_object("flag", templates["flag"], (0.38, -0.05, height * 0.95))
        paint(flag, cloth)
        parent(flag, root)
    elif prop == "rifle":
        rifle = mesh_object("rifle", templates["rifle"], (0.05, -0.08, height * 0.7))
        rifle.rotation_euler = (0.3, 0.0, 0.4)
        paint(rifle, black)
        parent(rifle, root)


def build_figure(person, templates, style, materials):
    height = max(1.2, float(person.get("height_m") or 1.7))
    color = tuple(person.get("color") or [180, 180, 180])
    key = color
    if key not in materials:
        materials[key] = shade(color, 0.5, "uniform")
        materials[("hat", key)] = shade(darker(color), 0.45, "hat")
        materials[("leg", key)] = shade(darker(color), 0.6, "leg")
        materials[("skin", key)] = shade((214, 170, 140), 0.55, "skin")
    root = bpy.data.objects.new(f"p{person['index']}", None)
    link(root)
    torso = mesh_object("torso", templates["torso"], (0.0, 0.0, height * 0.62))
    paint(torso, materials[key])
    parent(torso, root)
    head = mesh_object("head", templates["head"], (0.0, -0.02, height * 0.95))
    paint(head, materials[("skin", key)])
    parent(head, root)
    if style != "capsule":
        shako = mesh_object("shako", templates["shako"], (0.0, 0.0, height * 1.08))
        paint(shako, materials[("hat", key)])
        parent(shako, root)
        brim = mesh_object("brim", templates["brim"], (0.0, -0.04, height * 1.0))
        paint(brim, materials[("hat", key)])
        parent(brim, root)
    legs = []
    for side, name in ((-1.0, "leg-l"), (1.0, "leg-r")):
        leg = mesh_object(name, templates["leg"], (side * 0.09, 0.0, height * 0.58))
        paint(leg, materials[("leg", key)])
        parent(leg, root)
        legs.append(leg)
    add_prop(root, person.get("prop") or "brass", templates, color, height)
    return root, legs


def action_curves(obj):
    action = obj.animation_data.action if obj.animation_data else None
    if action is None:
        return []
    curves = list(getattr(action, "fcurves", None) or [])
    if not curves and hasattr(action, "layers"):
        for layer in action.layers:
            for strip in getattr(layer, "strips", []):
                slots = getattr(action, "slots", None)
                bag = None
                if slots and hasattr(strip, "channelbag"):
                    bag = strip.channelbag(slots[0])
                if bag is not None and getattr(bag, "fcurves", None):
                    curves.extend(bag.fcurves)
    return curves


def linear_keys(obj, data_path):
    for curve in action_curves(obj):
        if data_path and curve.data_path != data_path:
            continue
        for point in curve.keyframe_points:
            point.interpolation = "LINEAR"


def set_interpolation(obj, mode):
    for curve in action_curves(obj):
        for point in curve.keyframe_points:
            point.interpolation = mode
            if mode == "BEZIER":
                point.handle_left_type = "AUTO"
                point.handle_right_type = "AUTO"


def build_performers(scene):
    style = scene.get("figures") or "person"
    if style != "capsule":
        folder = script_dir()
        if folder not in sys.path:
            sys.path.insert(0, folder)
        import drill_marcher

        drill_marcher.build_show(scene)
        return {}
    field = scene["field"]
    templates = build_templates()
    materials = {}
    roots = {}
    legs = {}
    for person in scene["performers"]:
        root, pair = build_figure(person, templates, style, materials)
        roots[person["index"]] = root
        legs[person["index"]] = pair
    grouped = {}
    for sample in scene["keyframes"]:
        grouped.setdefault(sample["performer"], []).append(sample)
    for index, samples in grouped.items():
        root = roots.get(index)
        if root is None:
            continue
        samples.sort(key=lambda item: item["frame"])
        seen = set()
        pair = legs.get(index) or []
        for sample in samples:
            frame = int(sample["frame"])
            if frame in seen:
                continue
            seen.add(frame)
            root.location = to_world(field, sample["x"], sample["y"], 0.0)
            root.rotation_euler = (0.0, 0.0, float(sample.get("facing") or 0.0))
            root.keyframe_insert("location", frame=frame)
            root.keyframe_insert("rotation_euler", frame=frame)
            step = int(sample.get("step") or 0)
            swing = 0.42 if step == 0 else -0.42
            if sample.get("holding"):
                swing *= 0.7
            for leg_index, leg in enumerate(pair):
                sign = 1.0 if leg_index == 0 else -1.0
                leg.rotation_euler = (sign * swing, 0.0, 0.0)
                leg.keyframe_insert("rotation_euler", frame=frame)
        linear_keys(root, "location")
        linear_keys(root, "rotation_euler")
        for leg in pair:
            linear_keys(leg, "rotation_euler")
    return roots


def look_at(obj, target):
    direction = Vector(target) - obj.location
    if direction.length < 1e-4:
        return
    obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def add_camera(name, location, lens):
    camera = bpy.data.cameras.new(name)
    camera.lens = lens
    camera.clip_end = 500.0
    obj = bpy.data.objects.new(name, camera)
    link(obj)
    obj.location = location
    return obj


def fit_lens(distance, span_x, span_y, aspect):
    # 36mm wide sensor. Pick the tighter of the horizontal and vertical fits.
    lens_x = 36.0 * distance / max(span_x, 0.1)
    lens_y = (36.0 / max(aspect, 0.1)) * distance / max(span_y, 0.1)
    return max(14.0, min(lens_x, lens_y) * 0.9)


def build_cameras(scene):
    field = scene["field"]
    width = float(field["width"])
    height = float(field["height"])
    aspect = float(scene.get("width") or 1280) / float(scene.get("height") or 720)
    center = (0.0, 0.0, 1.4)
    distance = max(width, height) * 0.95
    press = add_camera(
        "press_box",
        (0.0, -(height * 0.5 + 14.0), max(16.0, height * 0.55)),
        28.0,
    )
    look_at(press, center)
    press.data.lens = fit_lens(press.location.length, width + 16.0, height + 18.0, aspect)
    end = add_camera(
        "end_zone",
        (width * 0.5 + 12.0, 0.0, max(8.0, height * 0.28)),
        28.0,
    )
    look_at(end, center)
    end.data.lens = fit_lens(end.location.length, height + 12.0, width * 0.5, aspect)
    orbit = add_camera("orbit", (0.0, -width * 0.55, height * 0.4), press.data.lens)
    frames = [int(item["frame"]) for item in scene["keyframes"]] or [1]
    start, end_frame = min(frames), max(frames)
    span = max(1, end_frame - start)
    radius = max(width, height) * 0.72
    for step in range(0, 17):
        frame = start + int(span * step / 16)
        angle = math.tau * step / 16.0
        orbit.location = (math.sin(angle) * radius, -math.cos(angle) * radius, height * 0.42)
        look_at(orbit, center)
        orbit.keyframe_insert("location", frame=frame)
        orbit.keyframe_insert("rotation_euler", frame=frame)
    set_interpolation(orbit, "BEZIER")
    follow = add_camera("follow", (0.0, -8.0, 6.0), 35.0)
    section = scene.get("follow_section") or ""
    wanted = [
        person["index"]
        for person in scene["performers"]
        if not section or person.get("section") == section
    ] or [person["index"] for person in scene["performers"]]
    grouped = {}
    for sample in scene["keyframes"]:
        if sample["performer"] in wanted:
            grouped.setdefault(int(sample["frame"]), []).append(sample)
    for frame, samples in sorted(grouped.items()):
        sx = sum(float(item["x"]) for item in samples) / len(samples)
        sy = sum(float(item["y"]) for item in samples) / len(samples)
        target = to_world(field, sx, sy, 1.3)
        follow.location = (target[0], target[1] - 11.0, 7.5)
        look_at(follow, target)
        follow.keyframe_insert("location", frame=frame)
        follow.keyframe_insert("rotation_euler", frame=frame)
    linear_keys(follow, "location")
    cameras = {
        "press_box": press,
        "end_zone": end,
        "orbit": orbit,
        "follow": follow,
    }
    if scene.get("cuts"):
        program = add_camera("program", press.location, press.data.lens)
        order = [press, end, follow, orbit]
        cues = scene.get("cues") or [{"frame": start}]
        for index, cue in enumerate(cues):
            source = order[index % len(order)]
            frame = int(cue["frame"])
            program.location = source.location
            program.rotation_euler = source.rotation_euler
            program.keyframe_insert("location", frame=frame)
            program.keyframe_insert("rotation_euler", frame=frame)
        set_interpolation(program, "CONSTANT")
        cameras["program"] = program
    return cameras


def clear_named(prefix):
    for obj in list(bpy.data.objects):
        if obj.name.startswith(prefix):
            bpy.data.objects.remove(obj, do_unlink=True)


def build_towers(field, night):
    """Four corner towers. The lamp banks glow only at night; the steel is always there."""
    width = float(field["width"])
    height = float(field["height"])
    corners = (
        (-8.0, -8.0),
        (width + 8.0, -8.0),
        (-8.0, height + 8.0),
        (width + 8.0, height + 8.0),
    )
    steel = shade((54, 58, 64), 0.45, "tower")
    lamp = shade((255, 214, 150) if night else (90, 92, 96), 0.3, "lamp")
    if night:
        lamp_node = lamp.node_tree.nodes.get("Principled BSDF")
        if lamp_node is not None:
            if "Emission Color" in lamp_node.inputs:
                lamp_node.inputs["Emission Color"].default_value = (1.0, 0.86, 0.62, 1.0)
            if "Emission Strength" in lamp_node.inputs:
                lamp_node.inputs["Emission Strength"].default_value = 6.0
    pole_mesh = cylinder_mesh(0.16, 16.0, 8)
    head_mesh = box_mesh(1.8, 0.7, 0.28)
    brace_mesh = box_mesh(0.08, 0.08, 4.2)
    for index, (x, y) in enumerate(corners):
        pole = mesh_object(f"lite-pole-{index}", pole_mesh, to_world(field, x, y, 8.0))
        paint(pole, steel)
        head = mesh_object(f"lite-head-{index}", head_mesh, to_world(field, x, y, 16.2))
        paint(head, lamp)
        for side, dx in enumerate((-0.55, 0.55)):
            brace = mesh_object(
                f"lite-brace-{index}-{side}",
                brace_mesh,
                to_world(field, x + dx, y, 6.5),
            )
            brace.rotation_euler = (0.0, math.radians(8 if dx > 0 else -8), 0.0)
            paint(brace, steel)


def build_lights(scene):
    clear_named("lite-")
    field = scene["field"]
    width = float(field["width"])
    height = float(field["height"])
    night = (scene.get("time_of_day") or "day") == "night"
    sky(night)
    build_towers(field, night)
    if night:
        corners = (
            (-8.0, -8.0),
            (width + 8.0, -8.0),
            (-8.0, height + 8.0),
            (width + 8.0, height + 8.0),
        )
        for index, (x, y) in enumerate(corners):
            # Sit the spot just inside the pole, clear of the mesh, aimed at the field.
            inward_x = width * 0.5 + (x - width * 0.5) * 0.92
            inward_y = height * 0.5 + (y - height * 0.5) * 0.92
            light = bpy.data.lights.new(f"lite-spot-{index}", "SPOT")
            light.energy = 6000
            light.color = (1.0, 0.93, 0.8)
            light.spot_size = math.radians(78)
            light.spot_blend = 0.4
            obj = bpy.data.objects.new(light.name, light)
            obj.location = to_world(field, inward_x, inward_y, 15.4)
            link(obj)
            look_at(obj, (0.0, 0.0, 1.0))
        wash = bpy.data.lights.new("lite-wash", "AREA")
        wash.energy = 200
        wash.color = (0.75, 0.82, 1.0)
        wash.size = 30.0
        wash_obj = bpy.data.objects.new("lite-wash", wash)
        wash_obj.location = (0.0, 0.0, 24.0)
        link(wash_obj)
        look_at(wash_obj, (0.0, 0.0, 0.0))
    else:
        sun = bpy.data.lights.new("lite-sun", "SUN")
        sun.energy = 1.15
        sun.color = (1.0, 0.97, 0.9)
        sun.angle = math.radians(2.5)
        obj = bpy.data.objects.new("lite-sun", sun)
        link(obj)
        obj.rotation_euler = (math.radians(52), math.radians(8), math.radians(28))
        fill = bpy.data.lights.new("lite-fill", "AREA")
        fill.energy = 140
        fill.color = (0.75, 0.84, 1.0)
        fill.size = 12.0
        fill_obj = bpy.data.objects.new("lite-fill", fill)
        fill_obj.location = (0.0, -height * 0.4, 14.0)
        link(fill_obj)
        look_at(fill_obj, (0.0, 0.0, 0.0))


def apply_engine(scene_data, samples):
    blend = bpy.context.scene
    engine = scene_data.get("engine") or "eevee"
    quality = scene_data.get("quality") or "draft"
    forced = int(samples or 0)
    fine = quality == "fine" or forced >= 24
    try:
        blend.view_settings.view_transform = "AgX"
    except TypeError:
        blend.view_settings.view_transform = "Filmic"
    if engine == "cycles":
        blend.render.engine = "CYCLES"
        blend.cycles.samples = max(1, samples)
        blend.cycles.device = "CPU"
        blend.render.use_persistent_data = False
        blend.cycles.use_denoising = False
        for view in blend.view_layers:
            cycles = getattr(view, "cycles", None)
            if cycles is not None and hasattr(cycles, "use_denoising"):
                cycles.use_denoising = False
        return
    for name in ("BLENDER_EEVEE_NEXT", "BLENDER_EEVEE"):
        try:
            blend.render.engine = name
            break
        except TypeError:
            continue
    eevee = getattr(blend, "eevee", None)
    if eevee is None:
        return
    if hasattr(eevee, "taa_render_samples"):
        eevee.taa_render_samples = forced if forced > 0 else (32 if fine else 8)
    if hasattr(eevee, "use_gtao"):
        eevee.use_gtao = fine
    if hasattr(eevee, "use_bloom"):
        eevee.use_bloom = fine
        if hasattr(eevee, "bloom_intensity"):
            eevee.bloom_intensity = 0.08
    if hasattr(eevee, "use_soft_shadows"):
        # Soft shadows on the fine pass. Draft keeps a hard sun shadow so 80 people stay near 2s/frame.
        eevee.use_soft_shadows = fine
    if hasattr(eevee, "shadow_cube_size"):
        eevee.shadow_cube_size = "1024" if fine else "512"


def apply_audio(scene_data, frame_start):
    path = scene_data.get("audio_path") or ""
    if not path or not os.path.isfile(path):
        return
    blend = bpy.context.scene
    if blend.sequence_editor is None:
        blend.sequence_editor_create()
    offset = float(scene_data.get("audio_offset_seconds") or 0.0)
    fps = int(blend.render.fps or 30)
    start = frame_start + int(round(offset * fps))
    editor = blend.sequence_editor
    try:
        editor.strips.new_sound("song", path, channel=1, frame_start=start)
    except AttributeError:
        editor.sequences.new_sound("song", path, 1, start)


def configure_render(scene_data, options):
    blend = bpy.context.scene
    frames = [int(item["frame"]) for item in scene_data["keyframes"]] or [1]
    start, end = min(frames), max(frames)
    fps = options.fps or int(scene_data.get("fps") or 30)
    if options.seconds and options.seconds > 0:
        end = min(end, start + int(options.seconds * fps))
    blend.frame_start = start
    blend.frame_end = max(start, end)
    blend.render.fps = fps
    blend.render.resolution_x = options.width or int(scene_data.get("width") or 1280)
    blend.render.resolution_y = options.height or int(scene_data.get("height") or 720)
    blend.render.resolution_percentage = 100
    blend.render.film_transparent = False
    blend.render.use_motion_blur = False
    samples = options.samples or int(scene_data.get("samples") or 8)
    if options.engine:
        scene_data["engine"] = options.engine
    apply_engine(scene_data, samples)
    apply_audio(scene_data, start)
    return start


_UI_FONT = None
_UI_FONT_READY = False
_CAPTION = None

# Draws the lower third as a flat RGBA image. The caption is not a mesh in the
# shot, so field geometry cannot cut through the glyphs.
_CAPTION_DRAW = r"""
import sys
from PIL import Image, ImageDraw, ImageFont


def load_font(path, size):
    font = ImageFont.truetype(path, size)
    try:
        font.set_variation_by_axes([700])
    except Exception:
        try:
            font.set_variation_by_name(b"Bold")
        except Exception:
            pass
    return font


def main():
    dest, text, width_s, height_s, font_path, mode = sys.argv[1:7]
    width, height = int(width_s), int(height_s)
    bar_h = max(8, int(round(height * 0.12)))
    margin = max(4, int(round(height * 0.025)))
    side = max(8, int(round(width * 0.04)))
    overlay = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)
    y1 = height - margin
    y0 = y1 - bar_h
    radius = max(6, int(round(bar_h * 0.22)))
    draw.rounded_rectangle((side, y0, width - side, y1), radius=radius, fill=(10, 12, 16, 188))
    stroke = 1
    target = int(bar_h * 0.56)
    max_w = width - 2 * side - int(bar_h * 0.5)
    font = load_font(font_path, 18)
    for size in range(18, 220):
        trial = load_font(font_path, size)
        box = draw.textbbox((0, 0), text, font=trial, stroke_width=stroke)
        ink_h = box[3] - box[1]
        ink_w = box[2] - box[0]
        if ink_h > target or ink_w > max_w:
            break
        font = trial
    box = draw.textbbox((0, 0), text, font=font, stroke_width=stroke)
    tw, th = box[2] - box[0], box[3] - box[1]
    tx = (width - tw) / 2.0 - box[0]
    ty = (y0 + y1 - th) / 2.0 - box[1]
    white = (255, 255, 255, 255)
    draw.text((tx, ty), text, font=font, fill=white, stroke_width=stroke, stroke_fill=white)
    if mode == "png":
        base = Image.open(dest).convert("RGBA")
        if base.size != (width, height):
            base = base.resize((width, height), Image.Resampling.LANCZOS)
        Image.alpha_composite(base, overlay).convert("RGB").save(dest)
    else:
        overlay.save(dest)
    print(f"caption {mode} {bar_h}px margin {margin}px {text}", flush=True)


if __name__ == "__main__":
    main()
"""


def font_file():
    """Japanese-capable face. The bundle ships NotoSansJP.ttf; system CJK is the fallback."""
    folder = script_dir()
    candidates = [
        os.path.join(folder, "NotoSansJP.ttf"),
        os.path.normpath(os.path.join(folder, "..", "..", "..", "assets", "NotoSansJP.ttf")),
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    ]
    for path in candidates:
        if os.path.isfile(path):
            return path
    return None


def ui_font():
    """Blender font for the yard numbers. The lower third does not use this."""
    global _UI_FONT, _UI_FONT_READY
    if _UI_FONT_READY:
        return _UI_FONT
    _UI_FONT_READY = True
    path = font_file()
    if path is None:
        print("ui font missing; title may show boxes", flush=True)
        return None
    try:
        font = bpy.data.fonts.load(path)
        print(f"ui font {path}", flush=True)
        _UI_FONT = font
        return font
    except RuntimeError as exc:
        print(f"ui font skipped {path}: {exc}", flush=True)
        return None


def add_overlay(scene, camera):
    """Remember the caption. It is painted on after the 3D render, in 2D."""
    global _CAPTION
    del camera
    if not scene.get("title_card", True):
        _CAPTION = None
        return
    path = font_file()
    if path:
        print(f"ui font {path}", flush=True)
    else:
        print("ui font missing; title may show boxes", flush=True)
    blend = bpy.context.scene
    fps = int(blend.render.fps or 24)
    intro = max(18, int(fps * 1.25))
    width = int(blend.render.resolution_x or 1280)
    height = int(blend.render.resolution_y or 720)
    _CAPTION = {
        "font": path or "",
        "title": scene.get("title") or "Drill",
        "cues": sorted(scene.get("cues") or [], key=lambda item: int(item["frame"])),
        "intro": intro,
        "width": width,
        "height": height,
    }
    print(
        f"overlay 2d frac 0.120 margin 0.025 intro {intro} {width}x{height}",
        flush=True,
    )


def caption_text(frame):
    state = _CAPTION
    if not state:
        return ""
    if int(frame) <= int(state["intro"]) or not state["cues"]:
        return state["title"]
    chosen = state["cues"][0]
    for cue in state["cues"]:
        if int(cue["frame"]) <= int(frame):
            chosen = cue
        else:
            break
    return f"セット{chosen.get('set', 1)} ・ {chosen.get('count', 0)}カウント"


def _draw_caption(dest, text, mode):
    state = _CAPTION
    if not state or not state.get("font") or not text:
        return False
    handle, script = tempfile.mkstemp(prefix="drill-caption-", suffix=".py")
    os.close(handle)
    try:
        with open(script, "w", encoding="utf-8") as out:
            out.write(_CAPTION_DRAW)
        subprocess.check_call(
            [
                "python3",
                script,
                dest,
                text,
                str(state["width"]),
                str(state["height"]),
                state["font"],
                mode,
            ]
        )
        return True
    finally:
        try:
            os.remove(script)
        except OSError:
            pass


def stamp_still(directory, name, frame):
    text = caption_text(frame)
    png = os.path.join(directory, name + ".png")
    if not text or not os.path.isfile(png):
        return
    try:
        _draw_caption(png, text, "png")
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"caption still failed {name}: {exc}", flush=True)


def stamp_movie(path):
    state = _CAPTION
    if not state or not state.get("font") or not os.path.isfile(path):
        return
    blend = bpy.context.scene
    start = int(blend.frame_start)
    end = int(blend.frame_end)
    ranges = []
    for frame in range(start, end + 1):
        text = caption_text(frame)
        if ranges and ranges[-1][2] == text:
            ranges[-1][1] = frame
        else:
            ranges.append([frame, frame, text])
    folder = tempfile.mkdtemp(prefix="drill-caption-")
    inputs = ["ffmpeg", "-y", "-i", path]
    filters = []
    previous = "0:v"
    try:
        for index, (first, last, text) in enumerate(ranges):
            plate = os.path.join(folder, f"{index}.png")
            _draw_caption(plate, text, "rgba")
            inputs.extend(["-i", plate])
            label = f"v{index}"
            # ffmpeg frame 0 is Blender frame `start`.
            enable = f"between(n,{first - start},{last - start})"
            filters.append(
                f"[{previous}][{index + 1}:v]overlay=0:0:format=auto:enable='{enable}'[{label}]"
            )
            previous = label
        stamped = path + ".captioned.mp4"
        command = inputs + [
            "-filter_complex",
            ";".join(filters),
            "-map",
            f"[{previous}]",
            "-map",
            "0:a?",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            "-c:a",
            "copy",
            "-movflags",
            "+faststart",
            stamped,
        ]
        print(f"caption movie {len(ranges)} plates", flush=True)
        subprocess.check_call(command)
        os.replace(stamped, path)
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"caption movie failed: {exc}", flush=True)
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def render_once(animation, samples):
    blend = bpy.context.scene
    try:
        if animation:
            bpy.ops.render.render(animation=True)
        else:
            bpy.ops.render.render(write_still=True)
    except Exception as exc:
        if blend.render.engine == "CYCLES":
            raise
        print(f"eevee failed ({exc}); falling back to cycles", flush=True)
        blend.render.engine = "CYCLES"
        blend.cycles.samples = max(1, int(samples or 8))
        blend.cycles.device = "CPU"
        blend.render.use_persistent_data = False
        blend.cycles.use_denoising = False
        if animation:
            bpy.ops.render.render(animation=True)
        else:
            bpy.ops.render.render(write_still=True)


def render_stills(cameras, directory, frame, samples):
    os.makedirs(directory, exist_ok=True)
    blend = bpy.context.scene
    blend.render.image_settings.file_format = "PNG"
    blend.frame_set(frame)
    for name, camera in cameras.items():
        if name == "program":
            continue
        blend.camera = camera
        blend.render.filepath = os.path.join(directory, name)
        print(f"Fra:{frame} still {name}", flush=True)
        render_once(False, samples)
        stamp_still(directory, name, frame)


def render_movie(camera, path, samples):
    blend = bpy.context.scene
    blend.camera = camera
    if blend.render.engine == "CYCLES":
        render_movie_from_stills(path, samples)
        return
    blend.render.image_settings.file_format = "FFMPEG"
    blend.render.ffmpeg.format = "MPEG4"
    blend.render.ffmpeg.codec = "H264"
    blend.render.ffmpeg.constant_rate_factor = "MEDIUM"
    blend.render.ffmpeg.audio_codec = "AAC"
    blend.render.filepath = path
    render_once(True, samples)
    stamp_movie(path)


def render_movie_from_stills(path, samples):
    blend = bpy.context.scene
    folder = path + ".frames"
    os.makedirs(folder, exist_ok=True)
    blend.render.image_settings.file_format = "PNG"
    start = int(blend.frame_start)
    end = int(blend.frame_end)
    for frame in range(start, end + 1):
        blend.frame_set(frame)
        blend.render.filepath = os.path.join(folder, f"{frame:04d}")
        print(f"Fra:{frame}", flush=True)
        render_once(False, samples)
    command = [
        "ffmpeg",
        "-y",
        "-framerate",
        str(int(blend.render.fps or 30)),
        "-start_number",
        str(start),
        "-i",
        os.path.join(folder, "%04d.png"),
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        path,
    ]
    subprocess.check_call(command)
    shutil.rmtree(folder, ignore_errors=True)
    stamp_movie(path)


def main():
    options = parse_args()
    if options.heroes_only:
        folder = script_dir()
        if folder not in sys.path:
            sys.path.insert(0, folder)
        import drill_marcher

        drill_marcher.render_heroes(options.heroes or os.path.join(folder, "heroes"))
        return
    data_path = options.data or os.path.join(script_dir(), "drill_scene.json")
    scene = load_scene(data_path)
    clear_scene()
    build_field(scene)
    build_performers(scene)
    cameras = build_cameras(scene)
    build_lights(scene)
    start = configure_render(scene, options)
    camera_name = options.camera or scene.get("camera") or "press_box"
    if scene.get("cuts") and "program" in cameras and not options.camera:
        camera_name = "program"
    chosen = cameras.get(camera_name) or cameras["press_box"]
    bpy.context.scene.camera = chosen
    add_overlay(scene, chosen)
    blend_path = os.path.join(script_dir(), "drill_scene.blend")
    bpy.ops.wm.save_as_mainfile(filepath=blend_path)
    samples = options.samples or int(scene.get("samples") or 8)
    # Past the intro card, so press_box and follow show the set/count bar.
    still_frame = start + int((bpy.context.scene.render.fps or 24) * 2)
    still_frame = min(still_frame, int(bpy.context.scene.frame_end))
    if options.stills:
        blend = bpy.context.scene
        blend.render.image_settings.file_format = "PNG"
        blend.frame_set(start)
        blend.camera = chosen
        blend.render.filepath = os.path.join(options.stills, "title")
        print(f"Fra:{start} still title", flush=True)
        render_once(False, samples)
        stamp_still(options.stills, "title", start)
        render_stills(cameras, options.stills, still_frame, samples)
    saved_time = scene.get("time_of_day") or "day"
    if options.night_stills:
        scene["time_of_day"] = "night"
        build_lights(scene)
        render_stills(cameras, options.night_stills, still_frame, samples)
        scene["time_of_day"] = saved_time
        if saved_time != "night":
            build_lights(scene)
    if options.render:
        bpy.context.scene.camera = chosen
        render_movie(chosen, options.render, samples)


if __name__ == "__main__":
    main()
