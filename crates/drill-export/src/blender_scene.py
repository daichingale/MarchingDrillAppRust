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
    curve.size = 2.4
    curve.align_x = "CENTER"
    curve.align_y = "CENTER"
    curve.extrude = 0.02
    curve.resolution_u = 6
    font = ui_font()
    if font is not None:
        curve.font = font
    height = float(field["height"])
    near = 3.2 if y is None else float(y)
    plate = mesh_object(
        f"num-plate-{text}-{x:.0f}",
        box_mesh(1.5, 0.08, 1.7),
        to_world(field, x, near - 0.15, 0.95),
    )
    paint(plate, shade((18, 28, 22), 0.8, "num-plate"))
    obj = bpy.data.objects.new(f"num-{text}-{x:.0f}", curve)
    # Tilt the digits up so the press box can read them. +Z faces the audience.
    obj.location = to_world(field, x, near, 0.95)
    obj.rotation_euler = (math.radians(68), 0.0, 0.0)
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
    paint(ground, shade((74, 92, 62), 0.95, "horizon"))
    border = mesh_object(
        "border",
        box_mesh(width + (18.0 if kind == "football" else 8.0), height + 14.0, 0.04),
        (0.0, 0.0, -0.02),
    )
    paint(border, track if kind == "football" else concrete)
    playing = mesh_object("field", box_mesh(width, height, 0.04), (0.0, 0.0, 0.0))
    paint(playing, turf)
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
            yard_label(field, float(meters), str(meters), grid_ink, height * 0.32)
    build_stands(field, kind)


def build_stands(field, kind):
    width = float(field["width"])
    height = float(field["height"])
    span = width * 0.7
    riser_mat = shade((58, 62, 70), 0.75, "riser")
    seat_mat = shade((96, 104, 118), 0.65, "seat")
    rail_mat = shade((214, 216, 210), 0.35, "rail")
    glass = shade((150, 186, 214), 0.15, "glass")
    frame = shade((48, 52, 58), 0.5, "booth")
    # Seat row on top of each riser, plus a front rail, on both sidelines.
    for side, y_edge, outward in (("south", -5.2, -1.0), ("north", height + 5.2, 1.0)):
        for row in range(6):
            depth = 1.15
            rise = 0.42
            y = y_edge + outward * (0.6 + row * 1.25)
            z = 0.28 + row * rise
            block = mesh_object(
                f"stand-{side}-{row}",
                box_mesh(span, depth, rise),
                to_world(field, width * 0.5, y, z),
            )
            paint(block, riser_mat)
            pad = mesh_object(
                f"seat-{side}-{row}",
                box_mesh(span * 0.98, 0.42, 0.08),
                to_world(field, width * 0.5, y + outward * 0.22, z + rise * 0.5 + 0.02),
            )
            paint(pad, seat_mat)
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


def stick_to_camera(obj, camera, location, rotation=(0.0, 0.0, 0.0)):
    obj.parent = camera
    obj.location = location
    obj.rotation_euler = rotation


_UI_FONT = None
_UI_FONT_READY = False


def ui_font():
    """Japanese-capable face. The bundle ships Noto Sans JP; system CJK is the fallback."""
    global _UI_FONT, _UI_FONT_READY
    if _UI_FONT_READY:
        return _UI_FONT
    _UI_FONT_READY = True
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
        if not os.path.isfile(path):
            continue
        try:
            font = bpy.data.fonts.load(path)
            print(f"ui font {path}", flush=True)
            _UI_FONT = font
            return font
        except RuntimeError as exc:
            print(f"ui font skipped {path}: {exc}", flush=True)
    print("ui font missing; title may show boxes", flush=True)
    return None


def add_overlay(scene, camera):
    if not scene.get("title_card", True):
        return
    font = ui_font()
    # Text already faces the lens: it sits on the camera's -Z and its front is +Z.
    facing = (0.0, 0.0, 0.0)
    plate = mesh_object("title-plate", box_mesh(2.4, 0.46, 0.02), (0.0, 0.0, 0.0))
    stick_to_camera(plate, camera, (0.0, 0.58, -1.9))
    paint(plate, shade((16, 20, 26), 0.8, "plate"))
    title = bpy.data.curves.new("title-card", "FONT")
    title.body = scene.get("title") or "Drill"
    title.size = 0.22
    title.align_x = "CENTER"
    title.align_y = "CENTER"
    title.extrude = 0.004
    title.resolution_u = 8
    if font is not None:
        title.font = font
    card = bpy.data.objects.new("title-card", title)
    stick_to_camera(card, camera, (0.0, 0.58, -1.82), facing)
    link(card)
    paint(card, shade((248, 244, 230), 0.4, "card-ink"))
    burn_plate = mesh_object("burn-plate", box_mesh(2.2, 0.20, 0.02), (0.0, 0.0, 0.0))
    stick_to_camera(burn_plate, camera, (0.0, -0.58, -1.9))
    paint(burn_plate, shade((16, 20, 26), 0.8, "burn-plate"))
    burn = bpy.data.curves.new("burn-in", "FONT")
    burn.body = ""
    burn.size = 0.07
    burn.align_x = "CENTER"
    burn.align_y = "CENTER"
    burn.extrude = 0.003
    burn.resolution_u = 8
    if font is not None:
        burn.font = font
    label = bpy.data.objects.new("burn-in", burn)
    stick_to_camera(label, camera, (0.0, -0.58, -1.82), facing)
    link(label)
    paint(label, shade((248, 244, 230), 0.4, "burn-ink"))
    cues = sorted(scene.get("cues") or [], key=lambda item: int(item["frame"]))
    show_name = scene.get("title") or ""
    fps = int(bpy.context.scene.render.fps or 24)

    def on_frame(blend, *_args):
        show = int(blend.frame_current) <= fps
        for name in ("title-card", "title-plate"):
            obj = bpy.data.objects.get(name)
            if obj is not None:
                obj.hide_render = not show
        obj = bpy.data.objects.get("burn-in")
        if obj is None or not cues:
            return
        chosen = cues[0]
        for cue in cues:
            if int(cue["frame"]) <= blend.frame_current:
                chosen = cue
            else:
                break
        obj.data.body = f"{show_name}    {chosen.get('set', 1)}    {chosen.get('count', 0)}"

    bpy.app.handlers.frame_change_pre.append(on_frame)
    on_frame(bpy.context.scene)


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


def hide_title():
    for name in ("title-card", "title-plate"):
        card = bpy.data.objects.get(name)
        if card:
            card.hide_render = True


def render_stills(cameras, directory, frame, samples):
    os.makedirs(directory, exist_ok=True)
    blend = bpy.context.scene
    blend.render.image_settings.file_format = "PNG"
    blend.frame_set(frame)
    hide_title()
    for name, camera in cameras.items():
        if name == "program":
            continue
        blend.camera = camera
        blend.render.filepath = os.path.join(directory, name)
        print(f"Fra:{frame} still {name}", flush=True)
        render_once(False, samples)


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
    still_frame = start + int(bpy.context.scene.render.fps or 24)
    still_frame = min(still_frame, int(bpy.context.scene.frame_end))
    if options.stills:
        blend = bpy.context.scene
        blend.render.image_settings.file_format = "PNG"
        blend.frame_set(start)
        blend.camera = chosen
        blend.render.filepath = os.path.join(options.stills, "title")
        print(f"Fra:{start} still title", flush=True)
        render_once(False, samples)
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
