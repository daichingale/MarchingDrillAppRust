# DrillForge 3D scene. Open in Blender, or render headless:
#   blender -b -P drill_scene.py -- --render out.mp4
# Optional: --data drill_scene.json --stills DIR --seconds 8 --samples 8
#           --engine eevee|cycles --camera press_box|end_zone|orbit|follow
#           --width 1920 --height 1080 --fps 30

import argparse
import json
import math
import os
import shutil
import subprocess
import sys

import bpy
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
    parser.add_argument("--seconds", type=float, default=0.0)
    parser.add_argument("--samples", type=int, default=0)
    parser.add_argument("--engine", default="")
    parser.add_argument("--camera", default="")
    parser.add_argument("--width", type=int, default=0)
    parser.add_argument("--height", type=int, default=0)
    parser.add_argument("--fps", type=int, default=0)
    return parser.parse_args(args_after_double_dash())


def script_dir():
    return os.path.dirname(os.path.abspath(__file__))


def load_scene(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def clear_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def principle(color, roughness=0.6):
    mat = bpy.data.materials.new("drill")
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


def world_color(rgba, strength):
    world = bpy.data.worlds.new("drill-world")
    bpy.context.scene.world = world
    world.use_nodes = True
    node = world.node_tree.nodes.get("Background")
    if node:
        node.inputs[0].default_value = rgba
        node.inputs[1].default_value = strength


def add_box(name, center, scale, material):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=center)
    obj = bpy.context.active_object
    obj.name = name
    obj.scale = scale
    obj.data.materials.append(material)
    return obj


def to_world(field, x, y, z):
    return (
        float(x) - float(field["width"]) * 0.5,
        float(y) - float(field["height"]) * 0.5,
        float(z),
    )


def build_field(scene):
    field = scene["field"]
    width = float(field["width"])
    height = float(field["height"])
    grass = principle([46, 140, 62], 0.92)
    line = principle([236, 236, 228], 0.4)
    frame_mat = principle([250, 214, 92], 0.45)
    bpy.ops.mesh.primitive_plane_add(size=2.0, location=(0.0, 0.0, 0.0))
    ground = bpy.context.active_object
    ground.name = "field"
    ground.scale = (width * 0.5, height * 0.5, 1.0)
    ground.data.materials.append(grass)
    z = 0.012
    for x in field.get("major_x", []):
        wx, wy, _ = to_world(field, x, height * 0.5, z)
        add_box(f"yard-x-{x:.1f}", (wx, wy, z), (0.04, height, 0.02), line)
    for y in field.get("major_y", []):
        wx, wy, _ = to_world(field, width * 0.5, y, z)
        add_box(f"yard-y-{y:.1f}", (wx, wy, z), (width, 0.04, 0.02), line)
    for index, bounds in enumerate(field.get("frames", [])):
        min_x, min_y, max_x, max_y = bounds
        cx = (min_x + max_x) * 0.5
        cy = (min_y + max_y) * 0.5
        span_x = max(0.2, max_x - min_x)
        span_y = max(0.2, max_y - min_y)
        for name, center, scale in (
            ("n", (cx, max_y, z), (span_x, 0.06, 0.025)),
            ("s", (cx, min_y, z), (span_x, 0.06, 0.025)),
            ("w", (min_x, cy, z), (0.06, span_y, 0.025)),
            ("e", (max_x, cy, z), (0.06, span_y, 0.025)),
        ):
            wx, wy, wz = to_world(field, center[0], center[1], center[2])
            add_box(f"frame-{index}-{name}", (wx, wy, wz), scale, frame_mat)


def join_active(parts):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in parts:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    if len(parts) > 1:
        bpy.ops.object.join()
    return bpy.context.active_object


def build_figure(person, style):
    height = max(1.2, float(person.get("height_m") or 1.7))
    color = person.get("color") or [180, 180, 180]
    mat = principle(color, 0.55)
    kind = person.get("kind") or "wind"
    parts = []
    if style == "person":
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0.0, 0.0, height * 0.42))
        torso = bpy.context.active_object
        torso.scale = (0.22, 0.12, height * 0.28)
        parts.append(torso)
        bpy.ops.mesh.primitive_uv_sphere_add(
            segments=8, ring_count=6, radius=0.13, location=(0.0, 0.0, height * 0.78)
        )
        parts.append(bpy.context.active_object)
        for side in (-1.0, 1.0):
            bpy.ops.mesh.primitive_cylinder_add(
                vertices=6,
                radius=0.05,
                depth=height * 0.42,
                location=(side * 0.1, 0.0, height * 0.2),
            )
            parts.append(bpy.context.active_object)
    else:
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=8,
            radius=0.16,
            depth=height * 0.62,
            location=(0.0, 0.0, height * 0.36),
        )
        parts.append(bpy.context.active_object)
        bpy.ops.mesh.primitive_uv_sphere_add(
            segments=8, ring_count=6, radius=0.14, location=(0.0, 0.0, height * 0.74)
        )
        parts.append(bpy.context.active_object)
    if kind == "guard":
        bpy.ops.mesh.primitive_plane_add(size=0.55, location=(0.32, 0.0, height * 0.85))
        flag = bpy.context.active_object
        flag.rotation_euler = (0.2, 0.5, 0.4)
        parts.append(flag)
    elif kind == "percussion":
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=8, radius=0.18, depth=0.12, location=(0.0, 0.22, height * 0.5)
        )
        drum = bpy.context.active_object
        drum.rotation_euler = (1.2, 0.0, 0.0)
        parts.append(drum)
    figure = join_active(parts)
    figure.name = f"p{person['index']}"
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    if hasattr(figure, "cycles"):
        figure.cycles.use_motion_blur = False
    if figure.data.materials:
        figure.data.materials[0] = mat
    else:
        figure.data.materials.append(mat)
    return figure


def linear_keys(obj):
    action = obj.animation_data.action if obj.animation_data else None
    if action is None:
        return
    curves = getattr(action, "fcurves", None)
    if curves is None and hasattr(action, "layers"):
        curves = []
        for layer in action.layers:
            for strip in layer.strips:
                channelbag = strip.channelbag(action.slots[0]) if action.slots else None
                if channelbag:
                    curves.extend(channelbag.fcurves)
    for curve in curves or []:
        for point in curve.keyframe_points:
            point.interpolation = "LINEAR"


def build_performers(scene, style):
    field = scene["field"]
    figures = {}
    for person in scene["performers"]:
        figures[person["index"]] = build_figure(person, style)
    by_person = {}
    for sample in scene["keyframes"]:
        by_person.setdefault(sample["performer"], []).append(sample)
    for index, samples in by_person.items():
        obj = figures.get(index)
        if obj is None:
            continue
        samples.sort(key=lambda item: item["frame"])
        seen = set()
        for sample in samples:
            frame = int(sample["frame"])
            if frame in seen:
                continue
            seen.add(frame)
            obj.location = to_world(field, sample["x"], sample["y"], 0.0)
            obj.keyframe_insert(data_path="location", frame=frame)
        linear_keys(obj)
    return figures


def look_at(obj, target):
    direction = Vector(target) - obj.location
    if direction.length < 1e-4:
        return
    obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def add_camera(name, location):
    camera = bpy.data.cameras.new(name)
    camera.lens = 28
    obj = bpy.data.objects.new(name, camera)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = location
    return obj


def build_cameras(scene, figures):
    field = scene["field"]
    width = float(field["width"])
    height = float(field["height"])
    center = (0.0, 0.0, 1.2)
    press = add_camera("press_box", (0.0, -height * 0.72, height * 0.42))
    look_at(press, center)
    end = add_camera("end_zone", (width * 0.62, 0.0, height * 0.28))
    look_at(end, center)
    orbit = add_camera("orbit", (0.0, -width * 0.55, height * 0.38))
    frames = [int(item["frame"]) for item in scene["keyframes"]] or [1]
    start, end_frame = min(frames), max(frames)
    span = max(1, end_frame - start)
    for step in range(0, 9):
        frame = start + int(span * step / 8)
        angle = math.tau * step / 8.0
        radius = max(width, height) * 0.62
        orbit.location = (math.sin(angle) * radius, -math.cos(angle) * radius, height * 0.36)
        look_at(orbit, center)
        orbit.keyframe_insert(data_path="location", frame=frame)
        orbit.keyframe_insert(data_path="rotation_euler", frame=frame)
    follow = add_camera("follow", (0.0, -8.0, 6.0))
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
        target = to_world(field, sx, sy, 1.2)
        follow.location = (target[0] - 7.0, target[1] - 9.0, 6.5)
        look_at(follow, target)
        follow.keyframe_insert(data_path="location", frame=frame)
        follow.keyframe_insert(data_path="rotation_euler", frame=frame)
    return {
        "press_box": press,
        "end_zone": end,
        "orbit": orbit,
        "follow": follow,
    }


def build_lights(time_of_day):
    if time_of_day == "night":
        world_color((0.01, 0.02, 0.05, 1.0), 0.15)
        spots = (
            ((-20.0, -15.0, 18.0), (0.75, 0.85, 1.0)),
            ((20.0, -15.0, 18.0), (0.75, 0.85, 1.0)),
            ((-20.0, 15.0, 16.0), (1.0, 0.85, 0.6)),
            ((20.0, 15.0, 16.0), (1.0, 0.85, 0.6)),
        )
        for index, (location, color) in enumerate(spots):
            light = bpy.data.lights.new(f"stadium-{index}", "SPOT")
            light.energy = 4000
            light.color = color
            light.spot_size = math.radians(55)
            obj = bpy.data.objects.new(light.name, light)
            bpy.context.scene.collection.objects.link(obj)
            obj.location = location
            look_at(obj, (0.0, 0.0, 0.0))
    else:
        world_color((0.55, 0.74, 1.0, 1.0), 1.0)
        sun = bpy.data.lights.new("day", "SUN")
        sun.energy = 4.5
        sun.color = (1.0, 0.96, 0.88)
        sun.angle = math.radians(2)
        obj = bpy.data.objects.new("day", sun)
        bpy.context.scene.collection.objects.link(obj)
        obj.rotation_euler = (math.radians(48), math.radians(8), math.radians(32))


def apply_engine(scene_data, samples):
    blend = bpy.context.scene
    engine = scene_data.get("engine") or "eevee"
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
    if eevee is not None and hasattr(eevee, "taa_render_samples"):
        eevee.taa_render_samples = max(1, samples)


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
    blend.render.resolution_x = options.width or int(scene_data.get("width") or 1920)
    blend.render.resolution_y = options.height or int(scene_data.get("height") or 1080)
    blend.render.resolution_percentage = 100
    blend.render.film_transparent = False
    blend.render.use_motion_blur = False
    samples = options.samples or int(scene_data.get("samples") or 32)
    if options.engine:
        scene_data["engine"] = options.engine
    apply_engine(scene_data, samples)
    apply_audio(scene_data, start)
    return start


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
        for view in blend.view_layers:
            cycles = getattr(view, "cycles", None)
            if cycles is not None and hasattr(cycles, "use_denoising"):
                cycles.use_denoising = False
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
        blend.camera = camera
        blend.render.filepath = os.path.join(directory, name)
        print(f"Fra:{frame} still {name}", flush=True)
        render_once(False, samples)


def render_movie(camera, path, samples):
    blend = bpy.context.scene
    blend.camera = camera
    # Cycles headless animation rebuilds a motion BVH and can crash Embree
    # (rtcSetSharedGeometryBuffer) after a few dozen frames. Still frames of
    # the same scene are stable, so the movie is those frames encoded to H.264.
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
    pattern = os.path.join(folder, "%04d.png")
    command = [
        "ffmpeg",
        "-y",
        "-framerate",
        str(int(blend.render.fps or 30)),
        "-start_number",
        str(start),
        "-i",
        pattern,
    ]
    audio = ""
    editor = blend.sequence_editor
    if editor is not None:
        strips = getattr(editor, "strips", None) or getattr(editor, "sequences", None)
        if strips:
            for strip in strips:
                sound = getattr(strip, "sound", None)
                filepath = getattr(sound, "filepath", "") if sound else ""
                if filepath and os.path.isfile(filepath):
                    audio = filepath
                    break
    if audio:
        command.extend(["-i", audio, "-shortest", "-c:a", "aac"])
    command.extend(
        ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", path]
    )
    subprocess.check_call(command)
    shutil.rmtree(folder, ignore_errors=True)


def main():
    options = parse_args()
    data_path = options.data or os.path.join(script_dir(), "drill_scene.json")
    scene = load_scene(data_path)
    clear_scene()
    build_field(scene)
    style = scene.get("figures") or "capsule"
    figures = build_performers(scene, style)
    cameras = build_cameras(scene, figures)
    build_lights(scene.get("time_of_day") or "day")
    start = configure_render(scene, options)
    camera_name = options.camera or scene.get("camera") or "press_box"
    chosen = cameras.get(camera_name) or cameras["press_box"]
    bpy.context.scene.camera = chosen
    blend_path = os.path.join(script_dir(), "drill_scene.blend")
    bpy.ops.wm.save_as_mainfile(filepath=blend_path)
    samples = options.samples or int(scene.get("samples") or 8)
    if options.stills:
        render_stills(cameras, options.stills, start, samples)
    if options.render:
        render_movie(chosen, options.render, samples)


if __name__ == "__main__":
    main()
