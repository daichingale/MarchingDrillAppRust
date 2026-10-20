# Rigged marching performer. Imported by drill_scene.py.
#
# One armature, MarcherRig, carries the actions march.forward, march.back,
# march.mark, march.attention, and march.horn. The show does not skin every
# person to that armature. Big casts (about 40 and up) reuse linked meshes and
# pose each limb with the same angles, which keeps 80–200 performers cheap.
# Feet plant on the count; the midpoint frame is the roll / passing step.
# Yaw 0 faces the audience. Forward march yaws into the direction of travel.

import math
import os

import bpy
import bmesh
from mathutils import Euler, Matrix, Vector

# About seven heads tall. Crown sits near 1.70 m; the shako and plume go above that.
HIP_Z = 0.90
KNEE_Z = 0.46
ANKLE_Z = 0.02
THIGH = HIP_Z - KNEE_Z
SHIN = KNEE_Z - ANKLE_Z
SHOULDER_Z = 1.36
SHOULDER_X = 0.19
UPPER = 0.28
FORE = 0.25
HEAD_Z = 1.58

JACKET = 0
SKIN = 1
HAT = 2
GOLD = 3
PLUME = 4
EYE = 5
MOUTH = 6
TROUSER = 7
SLOT_COUNT = 8

# Object-space swings. Negative X reaches toward the audience (-Y).
# Forward march is an 8-to-5 roll step: upright torso, short stride, heel
# down and toe up in front, the back knee nearly straight. Mark time lifts
# the knee and keeps the toe on the ground.
FORWARD_PLANT = {
    "thigh.L": (-0.28, 0.0, 0.02),
    "shin.L": (0.04, 0.0, 0.0),
    "foot.L": (-0.42, 0.0, 0.0),
    "thigh.R": (0.20, 0.0, -0.02),
    "shin.R": (0.16, 0.0, 0.0),
    "foot.R": (0.12, 0.0, 0.0),
    "arm.L": (0.32, 0.0, 0.10),
    "forearm.L": (0.22, 0.0, 0.0),
    "arm.R": (-0.36, 0.0, -0.10),
    "forearm.R": (0.20, 0.0, 0.0),
    "spine": (-0.02, 0.0, 0.0),
    "head": (0.02, 0.0, 0.0),
}
FORWARD_PASS = {
    "thigh.L": (0.04, 0.0, 0.01),
    "shin.L": (0.06, 0.0, 0.0),
    "foot.L": (0.08, 0.0, 0.0),
    "thigh.R": (-0.12, 0.0, -0.01),
    "shin.R": (0.38, 0.0, 0.0),
    "foot.R": (-0.15, 0.0, 0.0),
    "arm.L": (0.08, 0.0, 0.08),
    "forearm.L": (0.16, 0.0, 0.0),
    "arm.R": (-0.10, 0.0, -0.08),
    "forearm.R": (0.16, 0.0, 0.0),
    "spine": (-0.02, 0.0, 0.0),
    "head": (0.02, 0.0, 0.0),
}
BACK_PLANT = {
    "thigh.L": (0.26, 0.0, 0.02),
    "shin.L": (0.10, 0.0, 0.0),
    "foot.L": (0.40, 0.0, 0.0),
    "thigh.R": (-0.16, 0.0, -0.02),
    "shin.R": (0.08, 0.0, 0.0),
    "foot.R": (-0.20, 0.0, 0.0),
    "arm.L": (-0.22, 0.0, 0.10),
    "forearm.L": (0.16, 0.0, 0.0),
    "arm.R": (0.24, 0.0, -0.10),
    "forearm.R": (0.16, 0.0, 0.0),
    "spine": (0.0, 0.0, 0.0),
    "head": (0.0, 0.0, 0.0),
}
BACK_PASS = {
    "thigh.L": (0.04, 0.0, 0.01),
    "shin.L": (0.08, 0.0, 0.0),
    "foot.L": (0.06, 0.0, 0.0),
    "thigh.R": (0.14, 0.0, -0.01),
    "shin.R": (0.32, 0.0, 0.0),
    "foot.R": (0.2, 0.0, 0.0),
    "arm.L": (-0.06, 0.0, 0.08),
    "forearm.L": (0.12, 0.0, 0.0),
    "arm.R": (0.08, 0.0, -0.08),
    "forearm.R": (0.12, 0.0, 0.0),
    "spine": (0.0, 0.0, 0.0),
    "head": (0.0, 0.0, 0.0),
}
MARK_PLANT = {
    "thigh.L": (0.0, 0.0, 0.02),
    "shin.L": (0.04, 0.0, 0.0),
    "foot.L": (0.0, 0.0, 0.0),
    "thigh.R": (0.0, 0.0, -0.02),
    "shin.R": (0.04, 0.0, 0.0),
    "foot.R": (0.0, 0.0, 0.0),
    "arm.L": (0.06, 0.0, 0.12),
    "forearm.L": (0.10, 0.0, 0.0),
    "arm.R": (0.06, 0.0, -0.12),
    "forearm.R": (0.10, 0.0, 0.0),
    "spine": (0.0, 0.0, 0.0),
    "head": (0.0, 0.0, 0.0),
}
MARK_PASS = {
    "thigh.L": (-0.48, 0.0, 0.02),
    "shin.L": (0.92, 0.0, 0.0),
    "foot.L": (0.72, 0.0, 0.0),
    "thigh.R": (0.0, 0.0, -0.02),
    "shin.R": (0.04, 0.0, 0.0),
    "foot.R": (0.0, 0.0, 0.0),
    "arm.L": (0.16, 0.0, 0.12),
    "forearm.L": (0.14, 0.0, 0.0),
    "arm.R": (-0.08, 0.0, -0.10),
    "forearm.R": (0.10, 0.0, 0.0),
    "spine": (0.0, 0.0, 0.0),
    "head": (0.0, 0.0, 0.0),
}
ATTENTION = {
    "thigh.L": (0.0, 0.0, 0.02),
    "shin.L": (0.02, 0.0, 0.0),
    "foot.L": (0.0, 0.0, 0.0),
    "thigh.R": (0.0, 0.0, -0.02),
    "shin.R": (0.02, 0.0, 0.0),
    "foot.R": (0.0, 0.0, 0.0),
    "arm.L": (0.04, 0.0, 0.14),
    "forearm.L": (0.08, 0.0, 0.0),
    "arm.R": (0.04, 0.0, -0.14),
    "forearm.R": (0.08, 0.0, 0.0),
    "spine": (0.0, 0.0, 0.0),
    "head": (0.0, 0.0, 0.0),
}

POSES = {
    "forward": (FORWARD_PLANT, FORWARD_PASS),
    "back": (BACK_PLANT, BACK_PASS),
    "mark": (MARK_PLANT, MARK_PASS),
    "attention": (ATTENTION, ATTENTION),
    "horn": (MARK_PLANT, MARK_PASS),
}

# Hands stay on the instrument instead of swinging.
ALWAYS_AIMED = {"flag", "rifle", "snare", "tenor", "bass", "mallet", "tuba", "sax"}
# These stay aimed even while standing at attention.
AIM_AT_ATTENTION = {"flag", "tuba", "sax", "snare", "tenor", "bass", "mallet"}

HERO_SPECS = (
    ("trumpet", (196, 48, 40)),
    ("mellophone", (196, 132, 24)),
    ("trombone", (32, 92, 196)),
    ("baritone", (24, 122, 72)),
    ("tuba", (96, 48, 168)),
    ("clarinet", (64, 96, 168)),
    ("flute", (120, 156, 196)),
    ("sax", (196, 154, 48)),
    ("snare", (48, 52, 60)),
    ("tenor", (48, 52, 60)),
    ("bass", (48, 52, 60)),
    ("mallet", (72, 118, 78)),
    ("flag", (196, 56, 132)),
    ("rifle", (70, 78, 88)),
)


def link(obj, collection=None):
    (collection or bpy.context.scene.collection).objects.link(obj)
    return obj


def shade(color, roughness=0.5, metallic=0.0, name="marcher"):
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
        if "Metallic" in node.inputs:
            node.inputs["Metallic"].default_value = metallic
        for key in ("Specular", "Specular IOR Level"):
            if key in node.inputs:
                node.inputs[key].default_value = 0.28 if metallic < 0.5 else 0.5
                break
    return mat


class Materials:
    def __init__(self):
        self.cache = {}
        self.skin = shade((214, 168, 140), 0.62, 0.0, "skin")
        self.hat = shade((28, 30, 34), 0.45, 0.05, "hat")
        self.gold = shade((232, 188, 74), 0.32, 0.75, "gold")
        self.eye = shade((36, 30, 32), 0.4, 0.0, "eye")
        self.mouth = shade((168, 86, 86), 0.55, 0.0, "mouth")
        self.shoe = shade((22, 22, 26), 0.55, 0.02, "shoe")
        self.glove = shade((236, 230, 220), 0.7, 0.0, "glove")
        self.brass = shade((212, 170, 64), 0.28, 0.86, "brass-metal")
        self.silver = shade((214, 220, 226), 0.22, 0.9, "silver")
        self.black = shade((24, 24, 28), 0.4, 0.15, "black-prop")
        self.wood = shade((122, 74, 42), 0.6, 0.0, "wood")
        self.white = shade((240, 240, 236), 0.35, 0.05, "white")
        self.drum = shade((58, 60, 64), 0.45, 0.25, "drum-shell")
        self.shadow = shade((18, 26, 18), 0.95, 0.0, "contact")

    def uniform(self, color):
        return self._tint("jacket", color, 0.55, 0.0)

    def trousers(self, color):
        dark = tuple(max(0, min(255, int(channel * 0.38 + 12))) for channel in color)
        return self._tint("trouser", dark, 0.7, 0.0)

    def plume(self, color):
        bright = tuple(max(0, min(255, int(channel * 0.35 + 168))) for channel in color)
        return self._tint("plume", bright, 0.5, 0.0)

    def cloth(self, color):
        return self._tint("cloth", color, 0.65, 0.0)

    def _tint(self, role, color, roughness, metallic):
        key = (role, tuple(color), roughness, metallic)
        if key not in self.cache:
            self.cache[key] = shade(color, roughness, metallic, role)
        return self.cache[key]

    def slots(self, color):
        return [
            self.uniform(color),
            self.skin,
            self.hat,
            self.gold,
            self.plume(color),
            self.eye,
            self.mouth,
            self.trousers(color),
        ]


def paint(obj, material):
    data = obj.data
    if data is None or not hasattr(data, "materials"):
        return
    if len(data.materials) == 0:
        data.materials.append(None)
    if not obj.material_slots:
        return
    slot = obj.material_slots[0]
    slot.link = "OBJECT"
    slot.material = material


def paint_slots(obj, materials):
    mesh = obj.data
    while len(mesh.materials) < len(materials):
        mesh.materials.append(None)
    for index, material in enumerate(materials):
        slot = obj.material_slots[index]
        slot.link = "OBJECT"
        slot.material = material


def _tag(bm, existing, material):
    for face in bm.faces:
        if face not in existing:
            face.material_index = material
            if material == SKIN:
                face.smooth = True


def add_box(bm, size, center, material):
    existing = set(bm.faces)
    geom = bmesh.ops.create_cube(bm, size=1.0)
    sx, sy, sz = size
    cx, cy, cz = center
    for vert in geom["verts"]:
        vert.co.x = vert.co.x * sx + cx
        vert.co.y = vert.co.y * sy + cy
        vert.co.z = vert.co.z * sz + cz
    _tag(bm, existing, material)


def add_sphere(bm, radius, center, material, segments, scale=None):
    existing = set(bm.faces)
    geom = bmesh.ops.create_uvsphere(
        bm,
        u_segments=segments,
        v_segments=max(4, segments // 2),
        radius=radius,
        matrix=Matrix.Translation(center),
    )
    if scale is not None:
        sx, sy, sz = scale
        cx, cy, cz = center
        for vert in geom["verts"]:
            vert.co.x = cx + (vert.co.x - cx) * sx
            vert.co.y = cy + (vert.co.y - cy) * sy
            vert.co.z = cz + (vert.co.z - cz) * sz
    _tag(bm, existing, material)


def add_cyl(bm, radius, depth, center, material, segments, shift_z=0.0, radius_top=None):
    existing = set(bm.faces)
    geom = bmesh.ops.create_cone(
        bm,
        cap_ends=True,
        segments=segments,
        radius1=radius,
        radius2=radius if radius_top is None else radius_top,
        depth=depth,
        matrix=Matrix.Translation((center[0], center[1], center[2] + shift_z)),
    )
    _tag(bm, existing, material)
    return geom


def add_rod(bm, start, end, radius, material, segments):
    start = Vector(start)
    end = Vector(end)
    direction = end - start
    length = direction.length
    if length < 1e-4:
        return
    mid = (start + end) * 0.5
    quat = Vector((0.0, 0.0, 1.0)).rotation_difference(direction.normalized())
    matrix = Matrix.Translation(mid) @ quat.to_matrix().to_4x4()
    existing = set(bm.faces)
    bmesh.ops.create_cone(
        bm,
        cap_ends=True,
        segments=segments,
        radius1=radius,
        radius2=radius,
        depth=length,
        matrix=matrix,
    )
    _tag(bm, existing, material)


def new_mesh(name, builder):
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    builder(bm)
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    for face in bm.faces:
        face.smooth = True
    bm.to_mesh(mesh)
    bm.free()
    if mesh.polygons:
        mesh.polygons.foreach_set("use_smooth", [True] * len(mesh.polygons))
    return mesh


def cylinder_mesh(name, radius, depth, segments, origin="center", radius_top=None):
    top = radius if radius_top is None else radius_top

    def build(bm):
        geom = bmesh.ops.create_cone(
            bm,
            cap_ends=True,
            segments=segments,
            radius1=radius,
            radius2=top,
            depth=depth,
        )
        if origin == "top":
            for vert in geom["verts"]:
                vert.co.z -= depth * 0.5

    return new_mesh(name, build)


def shoe_mesh(name):
    def build(bm):
        # Rounded shoe. Origin is the ankle. Toe points toward the audience.
        add_sphere(bm, 0.048, (0.0, -0.07, -0.012), 0, 10, (0.95, 1.85, 0.42))
        add_sphere(bm, 0.040, (0.0, 0.02, -0.004), 0, 8, (0.9, 0.75, 0.38))

    return new_mesh(name, build)


def mitten_mesh(name):
    def build(bm):
        add_sphere(bm, 0.036, (0.0, -0.012, 0.0), 0, 8, (1.05, 1.45, 0.62))
        add_sphere(bm, 0.016, (0.028, -0.01, 0.006), 0, 6, (0.7, 1.1, 0.7))

    return new_mesh(name, build)


def body_mesh(detailed):
    seg = 12 if detailed else 6
    hat_seg = 12 if detailed else 6
    plume_n = 6 if detailed else 3

    def build(bm):
        # Trousers at the hips, then a tapered jacket, a narrow neck, and shoulders.
        add_sphere(bm, 0.118, (0.0, 0.0, 0.90), TROUSER, seg, (1.05, 0.78, 0.55))
        add_cyl(bm, 0.105, 0.40, (0.0, -0.01, 1.16), JACKET, hat_seg, radius_top=0.155)
        add_sphere(bm, 0.078, (-0.20, -0.01, 1.34), JACKET, seg, (1.0, 0.85, 0.8))
        add_sphere(bm, 0.078, (0.20, -0.01, 1.34), JACKET, seg, (1.0, 0.85, 0.8))
        add_cyl(bm, 0.128, 0.04, (0.0, 0.0, 0.98), GOLD, hat_seg)
        add_cyl(bm, 0.055, 0.10, (0.0, 0.0, 1.46), SKIN, 10)
        add_cyl(bm, 0.072, 0.04, (0.0, -0.015, 1.40), JACKET, 10, radius_top=0.055)
        add_cyl(bm, 0.064, 0.016, (0.0, -0.015, 1.422), GOLD, 10)
        if detailed:
            add_rod(bm, (-0.12, -0.09, 1.34), (0.07, -0.11, 0.99), 0.011, GOLD, 6)
            add_rod(bm, (0.12, -0.09, 1.34), (-0.07, -0.11, 0.99), 0.011, GOLD, 6)
            add_sphere(bm, 0.055, (-0.17, -0.02, 1.38), GOLD, 8, (1.25, 0.85, 0.32))
            add_sphere(bm, 0.055, (0.17, -0.02, 1.38), GOLD, 8, (1.25, 0.85, 0.32))
            for z in (1.10, 1.18, 1.26, 1.34):
                add_sphere(bm, 0.012, (0.0, -0.105, z), GOLD, 5)
        add_sphere(bm, 0.112, (0.0, -0.01, HEAD_Z), SKIN, seg, (0.94, 1.0, 1.08))
        add_sphere(bm, 0.018, (-0.038, -0.108, 1.598), EYE, 6 if detailed else 5)
        add_sphere(bm, 0.018, (0.038, -0.108, 1.598), EYE, 6 if detailed else 5)
        if detailed:
            add_sphere(bm, 0.016, (0.0, -0.118, 1.575), SKIN, 6, (0.7, 0.85, 0.7))
            add_sphere(bm, 0.009, (-0.038, -0.122, 1.600), EYE, 5)
            add_sphere(bm, 0.009, (0.038, -0.122, 1.600), EYE, 5)
            add_rod(bm, (-0.062, -0.105, 1.625), (-0.016, -0.112, 1.628), 0.005, EYE, 4)
            add_rod(bm, (0.016, -0.112, 1.628), (0.062, -0.105, 1.625), 0.005, EYE, 4)
            add_sphere(bm, 0.014, (0.0, -0.120, 1.532), MOUTH, 6, (1.7, 0.4, 0.45))
        # Shako: cylinder, forward brim, gold band, chin strap.
        add_cyl(bm, 0.112, 0.15, (0.0, 0.0, 1.80), HAT, hat_seg)
        add_cyl(bm, 0.118, 0.022, (0.0, 0.0, 1.745), GOLD, hat_seg)
        brim = add_cyl(bm, 0.150, 0.012, (0.0, -0.04, 1.735), HAT, hat_seg)
        for vert in brim["verts"]:
            if vert.co.y < -0.04:
                vert.co.y -= 0.045
        if detailed:
            add_rod(bm, (-0.09, -0.01, 1.76), (-0.035, -0.115, 1.50), 0.006, HAT, 5)
            add_rod(bm, (0.09, -0.01, 1.76), (0.035, -0.115, 1.50), 0.006, HAT, 5)
            add_rod(bm, (-0.035, -0.115, 1.50), (0.035, -0.115, 1.50), 0.006, HAT, 5)
        # Feather: a tapered curve, not a stack of beads.
        knots = []
        for index in range(plume_n):
            t = index / max(1, plume_n - 1)
            y = -0.02 + 0.22 * t + 0.12 * t * t
            z = 1.90 + 0.40 * t - 0.10 * t * t
            radius = 0.030 * (1.0 - t) ** 0.8 + 0.004
            knots.append((0.0, y, z, radius))
        for start, end in zip(knots, knots[1:]):
            add_rod(bm, start[:3], end[:3], (start[3] + end[3]) * 0.5, PLUME, 6 if detailed else 4)

    return new_mesh("marcher-body", build)


def flag_mesh(detailed):
    nx = 8 if detailed else 5
    ny = 6 if detailed else 4
    width = 0.72
    height = 0.46

    def build(bm):
        verts = []
        for j in range(ny):
            v = j / (ny - 1)
            for i in range(nx):
                u = i / (nx - 1)
                wave = math.sin(u * math.pi * 1.6) * (0.04 + 0.05 * v)
                # Height runs along -Z so it follows a pole aimed upward. Width flies to the side.
                verts.append((u * width, wave, -v * height))
        for vert in verts:
            bm.verts.new(vert)
        bm.verts.ensure_lookup_table()
        for j in range(ny - 1):
            for i in range(nx - 1):
                a = j * nx + i
                bm.faces.new(
                    (
                        bm.verts[a],
                        bm.verts[a + 1],
                        bm.verts[a + 1 + nx],
                        bm.verts[a + nx],
                    )
                )

    return new_mesh("flag-cloth", build)


def torus_mesh(major, minor, major_seg, minor_seg):
    mesh = bpy.data.meshes.new("torus")
    verts = []
    faces = []
    for i in range(major_seg):
        angle = math.tau * i / major_seg
        ca, sa = math.cos(angle), math.sin(angle)
        for j in range(minor_seg):
            beta = math.tau * j / minor_seg
            cb, sb = math.cos(beta), math.sin(beta)
            radius = major + minor * cb
            # Vertical ring around the torso: circle in the XZ plane.
            verts.append((radius * ca, minor * sb, radius * sa))
    for i in range(major_seg):
        for j in range(minor_seg):
            n0 = i * minor_seg + j
            n1 = i * minor_seg + (j + 1) % minor_seg
            m0 = ((i + 1) % major_seg) * minor_seg + j
            m1 = ((i + 1) % major_seg) * minor_seg + (j + 1) % minor_seg
            faces.append((n0, m0, m1, n1))
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    return mesh


def cone_mesh(name, radius_base, radius_tip, depth, segments):
    def build(bm):
        bmesh.ops.create_cone(
            bm,
            cap_ends=True,
            segments=segments,
            radius1=radius_base,
            radius2=radius_tip,
            depth=depth,
        )

    return new_mesh(name, build)


def unit_cube():
    return new_mesh("cube", lambda bm: add_box(bm, (1.0, 1.0, 1.0), (0.0, 0.0, 0.0), 0))


def shared_meshes(detailed):
    limb_seg = 8 if detailed else 5
    prop_seg = 10 if detailed else 6
    # Every person reuses these datablocks (linked duplicates). Only the
    # object transforms and the object-linked materials differ. Limbs taper
    # from the joint toward the wrist and the ankle.
    return {
        "body": body_mesh(detailed),
        "thigh": cylinder_mesh("thigh", 0.048, THIGH, limb_seg, "top", 0.078),
        "shin": cylinder_mesh("shin", 0.034, SHIN, limb_seg, "top", 0.050),
        "shoe": shoe_mesh("shoe"),
        "arm": cylinder_mesh("arm", 0.034, UPPER, limb_seg, "top", 0.050),
        "fore": cylinder_mesh("fore", 0.026, FORE, limb_seg, "top", 0.036),
        "glove": mitten_mesh("glove"),
        "flag": flag_mesh(detailed),
        "sousa": torus_mesh(0.30, 0.055, 14 if detailed else 8, 6 if detailed else 4),
        "pipe": cylinder_mesh("pipe", 0.012, 0.42, prop_seg, "top"),
        "pipe_long": cylinder_mesh("pipe-long", 0.011, 0.68, prop_seg, "top"),
        "slim": cylinder_mesh("slim", 0.008, 0.5, prop_seg, "top"),
        "bell": cone_mesh("bell", 0.062, 0.012, 0.14, prop_seg),
        "stick": cylinder_mesh("stick", 0.006, 0.34, 5, "top"),
        "pole": cylinder_mesh("pole", 0.012, 1.2, 6, "top"),
        "barrel": cylinder_mesh("barrel", 0.013, 0.72, 6, "top"),
        "stock": unit_cube(),
        "cube": unit_cube(),
        "bead": cylinder_mesh("bead", 0.022, 0.05, 6, "center"),
        "snare": cylinder_mesh("snare", 0.15, 0.11, prop_seg, "center"),
        "rim": cylinder_mesh("rim", 0.158, 0.02, prop_seg, "center"),
        "tenor": cylinder_mesh("tenor", 0.09, 0.1, prop_seg, "center"),
        "bassdrum": cylinder_mesh("bassdrum", 0.26, 0.16, prop_seg, "center"),
        "detailed": detailed,
    }


def empty(name, collection=None):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_size = 0.08
    obj.rotation_mode = "XYZ"
    return link(obj, collection)


def mesh_obj(name, mesh, collection=None):
    obj = bpy.data.objects.new(name, mesh)
    obj.rotation_mode = "XYZ"
    return link(obj, collection)


def adopt(child, parent, location, rotation=(0.0, 0.0, 0.0)):
    child.parent = parent
    child.location = location
    child.rotation_euler = rotation
    child.matrix_parent_inverse = Matrix.Identity(4)
    return child


def mirror_name(name):
    if name.endswith(".L"):
        return name[:-2] + ".R"
    if name.endswith(".R"):
        return name[:-2] + ".L"
    return name


def mirror_pose(pose):
    return {mirror_name(name): (rx, -ry, -rz) for name, (rx, ry, rz) in pose.items()}


def euler_pointing(direction):
    direction = Vector(direction)
    if direction.length < 1e-6:
        return (0.0, 0.0, 0.0)
    quat = Vector((0.0, 0.0, -1.0)).rotation_difference(direction.normalized())
    euler = quat.to_euler("XYZ")
    return (euler.x, euler.y, euler.z)


def rotate_vec(vec, axis, angle):
    c = math.cos(angle)
    s = math.sin(angle)
    return vec * c + axis.cross(vec) * s + axis * axis.dot(vec) * (1.0 - c)


def solve_two_bone(shoulder, target, pole, length_a, length_b):
    shoulder = Vector(shoulder)
    target = Vector(target)
    offset = target - shoulder
    dist = offset.length
    reach = length_a + length_b - 1e-3
    if dist < 1e-4:
        offset = Vector((0.0, -1.0, 0.0))
        dist = 1.0
    if dist > reach:
        target = shoulder + offset.normalized() * reach
        dist = reach
    cos_angle = (length_a * length_a + dist * dist - length_b * length_b) / (2.0 * length_a * dist)
    cos_angle = max(-1.0, min(1.0, cos_angle))
    angle = math.acos(cos_angle)
    to_target = (target - shoulder).normalized()
    pole_vec = Vector(pole) - shoulder
    reject = pole_vec - to_target * pole_vec.dot(to_target)
    if reject.length < 1e-5:
        reject = Vector((0.0, -1.0, 0.0))
    reject.normalize()
    axis = to_target.cross(reject)
    if axis.length < 1e-5:
        axis = Vector((1.0, 0.0, 0.0))
    axis.normalize()
    upper_dir = rotate_vec(to_target, axis, angle).normalized()
    elbow = shoulder + upper_dir * length_a
    fore_dir = (target - elbow).normalized()
    arm_euler = euler_pointing(upper_dir)
    arm_rot = Euler(arm_euler, "XYZ").to_matrix()
    fore_euler = euler_pointing(arm_rot.inverted() @ fore_dir)
    return arm_euler, fore_euler


def shoulder(side):
    return (side * SHOULDER_X, 0.0, SHOULDER_Z)


def solve_arms(targets, pole_xyz):
    """targets: dict with optional 'L' and 'R' world points."""
    pose = {}
    px, py, pz = pole_xyz
    for side_name, sign in (("L", -1.0), ("R", 1.0)):
        if side_name not in targets:
            continue
        # Poles sit out to the side so the elbows leave the ribs.
        pole = (sign * px, py, pz)
        arm_euler, fore_euler = solve_two_bone(
            shoulder(sign), targets[side_name], pole, UPPER, FORE
        )
        pose[f"arm.{side_name}"] = arm_euler
        pose[f"forearm.{side_name}"] = fore_euler
    return pose


def arm_targets(prop, gait):
    order = gait == "attention"
    if prop == "rifle" and order:
        return {"R": (0.26, -0.06, 0.78), "L": (0.18, -0.12, 1.05)}
    aimed = prop in ALWAYS_AIMED
    if prop not in ALWAYS_AIMED:
        aimed = gait == "horn"
    elif order and prop not in AIM_AT_ATTENTION:
        aimed = False
    if not aimed:
        return None
    # Horns: mouthpiece at the lips, bell hand further out, elbows wide.
    # Drums sit at the waist. The flag pole is vertical between both hands.
    table = {
        "trumpet": {"R": (0.03, -0.16, 1.50), "L": (-0.02, -0.32, 1.48)},
        "mellophone": {"R": (0.02, -0.16, 1.50), "L": (0.10, -0.30, 1.44)},
        "trombone": {"R": (0.02, -0.16, 1.50), "L": (-0.16, -0.34, 1.42)},
        "baritone": {"R": (0.06, -0.18, 1.28), "L": (-0.04, -0.20, 1.36)},
        "clarinet": {"L": (0.02, -0.16, 1.48), "R": (0.03, -0.18, 1.24)},
        "flute": {"R": (0.16, -0.16, 1.50), "L": (-0.10, -0.16, 1.50)},
        "sax": {"R": (0.08, -0.20, 1.16), "L": (0.02, -0.18, 1.40)},
        "tuba": {"L": (0.0, -0.18, 1.40), "R": (0.14, -0.22, 1.12)},
        "snare": {"R": (0.14, -0.32, 1.02), "L": (-0.14, -0.32, 1.02)},
        "tenor": {"R": (0.14, -0.32, 1.08), "L": (-0.14, -0.32, 1.08)},
        "bass": {"R": (0.18, -0.36, 1.02), "L": (-0.18, -0.36, 1.02)},
        "mallet": {"R": (0.16, -0.40, 1.08), "L": (-0.16, -0.40, 1.08)},
        "flag": {"L": (0.08, -0.14, 1.12), "R": (0.08, -0.12, 1.52)},
        "rifle": {"R": (0.04, -0.22, 1.22), "L": (-0.12, -0.20, 1.08)},
    }
    return table.get(prop)


# Elbow pole, in character space. X is mirrored per arm.
POLES = {
    "trumpet": (0.34, -0.06, 1.40),
    "mellophone": (0.32, -0.08, 1.38),
    "trombone": (0.38, -0.12, 1.30),
    "baritone": (0.30, -0.14, 1.22),
    "clarinet": (0.26, -0.10, 1.34),
    "flute": (0.22, -0.04, 1.48),
    "sax": (0.28, -0.14, 1.22),
    "tuba": (0.32, -0.10, 1.22),
    "snare": (0.20, -0.18, 1.10),
    "tenor": (0.22, -0.20, 1.08),
    "bass": (0.26, -0.22, 1.02),
    "mallet": (0.24, -0.24, 1.10),
    "flag": (0.16, -0.06, 1.36),
    "rifle": (0.20, -0.14, 1.18),
}


_ARM_CACHE = {}


def aimed_arms(prop, gait):
    targets = arm_targets(prop, gait)
    if not targets:
        return None
    pole = POLES.get(prop, (0.30, -0.12, 1.25))
    key = (prop, gait, pole, tuple(sorted((name, tuple(point)) for name, point in targets.items())))
    if key not in _ARM_CACHE:
        _ARM_CACHE[key] = solve_arms(targets, pole)
    return _ARM_CACHE[key]


def pose_for(gait, step, phase, prop):
    gait = gait if gait in POSES else "forward"
    plant, passing = POSES[gait]
    pose = dict(plant if phase < 0.25 else passing)
    if int(step) % 2 == 1:
        pose = mirror_pose(pose)
    override = aimed_arms(prop, gait)
    if override:
        pose.update(override)
    return pose


def resolve_prop(prop, index):
    if prop == "drum":
        return ("snare", "tenor", "bass")[int(index) % 3]
    if prop == "woodwind":
        return "clarinet"
    if prop == "brass":
        return "trumpet"
    if prop in (
        "trumpet",
        "mellophone",
        "trombone",
        "baritone",
        "tuba",
        "clarinet",
        "flute",
        "sax",
        "snare",
        "tenor",
        "bass",
        "mallet",
        "flag",
        "rifle",
        "none",
    ):
        return prop
    return "trumpet"


def fore_matrix(pose, side):
    arm = Euler(pose[f"arm.{side}"], "XYZ").to_matrix()
    fore = Euler(pose[f"forearm.{side}"], "XYZ").to_matrix()
    return arm @ fore


def grip_euler(pose, side, world_dir):
    local = fore_matrix(pose, side).inverted() @ Vector(world_dir).normalized()
    return euler_pointing(local)


def add_tube(parent, mesh, location, rotation, material, collection):
    obj = mesh_obj("prop", mesh, collection)
    paint(obj, material)
    adopt(obj, parent, location, rotation)
    return obj


def attach_props(person_prop, parts, meshes, mats, color, collection, detailed):
    """Parent instruments to the hand or the spine. Meshes are shared."""
    del detailed  # segment count is already baked into the shared meshes
    spine = parts["spine"]
    right = parts["forearm.R"]
    left = parts["forearm.L"]
    hand_r = (0.0, -0.02, -FORE)
    hand_l = (0.0, -0.02, -FORE)
    prop = person_prop
    parts["grips"] = []

    def tube(parent, hand, material, mesh):
        obj = add_tube(parent, mesh, hand, (0.0, 0.0, 0.0), material, collection)
        parts["grips"].append(obj)
        return obj

    if prop in ("trumpet", "mellophone", "trombone", "baritone", "clarinet", "flute"):
        metal = mats.brass
        if prop == "flute":
            metal = mats.silver
        elif prop == "clarinet":
            metal = mats.black
        parent = left if prop == "clarinet" else right
        hand = hand_l if prop == "clarinet" else hand_r
        mesh = meshes["slim"] if prop in ("clarinet", "flute") else meshes["pipe"]
        if prop == "trombone":
            mesh = meshes["pipe_long"]
        obj = tube(parent, hand, metal, mesh)
        bell_obj = mesh_obj("bell", meshes["bell"], collection)
        paint(bell_obj, mats.silver if prop == "clarinet" else metal)
        end = -0.66 if prop == "trombone" else -0.48 if prop == "flute" else -0.40
        if prop == "trumpet":
            bell_obj.scale = (1.8, 1.8, 1.35)
        adopt(bell_obj, obj, (0.0, 0.0, end))
        obj["side"] = "L" if prop == "clarinet" else "R"
        directions = {
            # Trumpet bell sits on the horizon, straight ahead.
            "trumpet": (0.0, -1.0, 0.04),
            "mellophone": (0.05, -0.96, 0.12),
            "trombone": (-0.15, -0.95, 0.02),
            "baritone": (0.0, -0.35, 0.90),
            "clarinet": (0.0, -0.12, -1.0),
            "flute": (1.0, -0.08, 0.02),
        }
        obj["bell_dir"] = directions[prop]
    elif prop == "tuba":
        horn = mesh_obj("sousa", meshes["sousa"], collection)
        paint(horn, mats.brass)
        adopt(horn, spine, (0.0, -0.04, 1.05))
        neck = mesh_obj("lead", meshes["pipe_long"], collection)
        paint(neck, mats.brass)
        adopt(neck, spine, (-0.02, -0.16, 1.48), euler_pointing((0.0, -0.35, 0.94)))
        bell_obj = mesh_obj("bell", meshes["bell"], collection)
        bell_obj.scale = (4.8, 4.8, 2.4)
        paint(bell_obj, mats.brass)
        # Wide end of the cone is local -Z. Aim it forward and up, clear of the shako.
        adopt(bell_obj, spine, (0.04, -0.55, 1.98), euler_pointing((0.08, -0.72, 0.68)))
        mouth = tube(left, hand_l, mats.brass, meshes["slim"])
        mouth["side"] = "L"
        mouth["bell_dir"] = (0.0, -0.35, 0.55)
    elif prop == "sax":
        horn = mesh_obj("sax", meshes["sousa"], collection)
        paint(horn, mats.brass)
        horn.scale = (0.42, 0.38, 0.58)
        adopt(horn, spine, (0.07, -0.18, 1.10), (0.35, 0.0, 0.25))
        bell_obj = mesh_obj("bell", meshes["bell"], collection)
        bell_obj.scale = (1.6, 1.6, 1.6)
        paint(bell_obj, mats.brass)
        adopt(bell_obj, spine, (0.02, -0.30, 0.78), (math.radians(115), 0.0, 0.0))
    elif prop in ("snare", "tenor", "bass"):
        build_drums(prop, spine, right, left, hand_r, hand_l, meshes, mats, collection, parts)
    elif prop == "mallet":
        board = mesh_obj("board", meshes["cube"], collection)
        board.scale = (0.62, 0.22, 0.06)
        paint(board, mats.wood)
        adopt(board, spine, (0.0, -0.38, 0.98))
        rail = mesh_obj("rail", meshes["cube"], collection)
        rail.scale = (0.64, 0.04, 0.03)
        paint(rail, mats.white)
        adopt(rail, board, (0.0, -0.55, 0.7))
        for side, parent, hand in (("R", right, hand_r), ("L", left, hand_l)):
            mallet = tube(parent, hand, mats.wood, meshes["stick"])
            head = mesh_obj("bead", meshes["bead"], collection)
            paint(head, mats.white)
            adopt(head, mallet, (0.0, 0.0, -0.34))
            mallet["side"] = side
            mallet["bell_dir"] = (0.0, -0.35, -0.85)
    elif prop == "flag":
        # Lower hand holds the pole. -Z aims up, so the cloth hangs above both hands.
        pole = tube(left, hand_l, mats.white, meshes["pole"])
        cloth = mesh_obj("flag", meshes["flag"], collection)
        paint(cloth, mats.cloth(color))
        adopt(cloth, pole, (0.015, 0.0, -0.42))
        stripe = mesh_obj("stripe", meshes["cube"], collection)
        stripe.scale = (0.04, 0.02, 0.46)
        paint(stripe, mats.gold)
        adopt(stripe, cloth, (0.05, 0.0, -0.23))
        pole["side"] = "L"
        pole["bell_dir"] = (0.0, -0.04, 1.0)
        parts["flap"] = cloth
    elif prop == "rifle":
        barrel = tube(right, hand_r, mats.black, meshes["barrel"])
        stock = mesh_obj("stock", meshes["stock"], collection)
        stock.scale = (0.045, 0.055, 0.26)
        paint(stock, mats.wood)
        adopt(stock, barrel, (0.0, 0.02, 0.12))
        barrel["side"] = "R"
        barrel["bell_dir"] = (-0.55, -0.5, -0.12)
        barrel["order_dir"] = (0.04, -0.08, -1.0)


def build_drums(prop, spine, right, left, hand_r, hand_l, meshes, mats, collection, parts):
    if prop == "snare":
        drum = mesh_obj("snare", meshes["snare"], collection)
        paint(drum, mats.white)
        adopt(drum, spine, (0.0, -0.28, 0.86), (math.radians(90), 0.0, 0.0))
        rim = mesh_obj("rim", meshes["rim"], collection)
        paint(rim, mats.silver)
        adopt(rim, drum, (0.0, 0.0, 0.05))
    elif prop == "tenor":
        spots = (
            (-0.13, -0.24, 1.02, 1.05),
            (0.13, -0.24, 1.02, 1.05),
            (0.0, -0.32, 0.98, 0.85),
            (0.0, -0.16, 1.16, 0.7),
        )
        for x, y, z, scale in spots:
            drum = mesh_obj("tenor", meshes["tenor"], collection)
            drum.scale = (scale, scale, 1.0)
            paint(drum, mats.white)
            adopt(drum, spine, (x, y, z))
    else:
        drum = mesh_obj("bass", meshes["bassdrum"], collection)
        paint(drum, mats.white)
        adopt(drum, spine, (0.0, -0.32, 0.78), (0.0, math.radians(90), 0.0))
    for side in (-0.08, 0.08):
        band = mesh_obj("strap", meshes["cube"], collection)
        band.scale = (0.03, 0.025, 0.42)
        paint(band, mats.black)
        adopt(band, spine, (side, -0.06, 1.24), (0.25, 0.0, 0.12 if side > 0 else -0.12))
    for side, parent, hand in (("R", right, hand_r), ("L", left, hand_l)):
        rod = add_tube(parent, meshes["stick"], hand, (0.0, 0.0, 0.0), mats.wood, collection)
        rod["side"] = side
        rod["bell_dir"] = (0.05 if side == "R" else -0.05, -0.15, -1.0)
        parts["grips"].append(rod)


def spawn(name, color, prop, height_m, meshes, mats, collection=None, detailed=False):
    scale = max(0.7, float(height_m or 1.75)) / 1.75
    root = empty(name, collection)
    spine = empty(f"{name}.spine", collection)
    adopt(spine, root, (0.0, 0.0, 0.0))
    body = mesh_obj(f"{name}.body", meshes["body"], collection)
    paint_slots(body, mats.slots(color))
    adopt(body, spine, (0.0, 0.0, 0.0))
    parts = {"root": root, "spine": spine, "body": body}
    trouser = mats.trousers(color)
    jacket = mats.uniform(color)
    for side, sign in (("L", -1.0), ("R", 1.0)):
        thigh = mesh_obj(f"{name}.thigh.{side}", meshes["thigh"], collection)
        paint(thigh, trouser)
        adopt(thigh, spine, (sign * 0.09, 0.0, HIP_Z))
        shin = mesh_obj(f"{name}.shin.{side}", meshes["shin"], collection)
        paint(shin, trouser)
        adopt(shin, thigh, (0.0, 0.0, -THIGH))
        foot = mesh_obj(f"{name}.foot.{side}", meshes["shoe"], collection)
        paint(foot, mats.shoe)
        adopt(foot, shin, (0.0, 0.0, -SHIN))
        arm = mesh_obj(f"{name}.arm.{side}", meshes["arm"], collection)
        paint(arm, jacket)
        adopt(arm, spine, (sign * SHOULDER_X, 0.0, SHOULDER_Z))
        fore = mesh_obj(f"{name}.forearm.{side}", meshes["fore"], collection)
        paint(fore, jacket)
        adopt(fore, arm, (0.0, 0.0, -UPPER))
        glove = mesh_obj(f"{name}.glove.{side}", meshes["glove"], collection)
        paint(glove, mats.glove)
        adopt(glove, fore, (0.0, -0.015, -FORE))
        parts[f"thigh.{side}"] = thigh
        parts[f"shin.{side}"] = shin
        parts[f"foot.{side}"] = foot
        parts[f"arm.{side}"] = arm
        parts[f"forearm.{side}"] = fore
    if prop and prop != "none":
        attach_props(prop, parts, meshes, mats, color, collection, detailed)
    parts.setdefault("grips", [])
    root.scale = (scale, scale, scale)
    parts["prop"] = prop
    parts["detailed"] = detailed
    return parts


def apply_pose(parts, pose, prop_dirs=None):
    for name in (
        "spine",
        "thigh.L",
        "shin.L",
        "foot.L",
        "thigh.R",
        "shin.R",
        "foot.R",
        "arm.L",
        "forearm.L",
        "arm.R",
        "forearm.R",
    ):
        if name in parts and name in pose:
            parts[name].rotation_euler = pose[name]
    order = bool(prop_dirs and prop_dirs.get("order"))
    for grip in parts.get("grips") or []:
        if "bell_dir" not in grip:
            continue
        side = grip.get("side", "R")
        direction = grip["order_dir"] if order and "order_dir" in grip else grip["bell_dir"]
        if side in ("L", "R") and f"arm.{side}" in pose and f"forearm.{side}" in pose:
            grip.rotation_euler = grip_euler(pose, side, direction)


def sample_gait(sample):
    gait = sample.get("gait") or ""
    if gait in POSES:
        return gait
    if sample.get("holding"):
        return "horn"
    return "forward"


def to_world(field, x, y, z):
    return (
        float(x) - float(field["width"]) * 0.5,
        float(y) - float(field["height"]) * 0.5,
        float(z),
    )


class Baker:
    def __init__(self, obj):
        self.obj = obj
        self.channels = {}

    def key(self, data_path, index, frame, value):
        bucket = self.channels.setdefault((data_path, index), {})
        bucket[int(frame)] = float(value)

    def write(self):
        if not any(self.channels.values()):
            return
        if self.obj.animation_data is None:
            self.obj.animation_data_create()
        action = bpy.data.actions.new((self.obj.name or "march")[:40] + ".act")
        self.obj.animation_data.action = action
        fcurves = getattr(action, "fcurves", None)
        if fcurves is None:
            self._write_operators()
            return
        for (data_path, index), keys in self.channels.items():
            frames = sorted(keys)
            if not frames:
                continue
            curve = fcurves.new(data_path, index=index)
            curve.keyframe_points.add(len(frames))
            for point_index, frame in enumerate(frames):
                point = curve.keyframe_points[point_index]
                point.co = (float(frame), keys[frame])
                point.interpolation = "LINEAR"
            curve.update()

    def _write_operators(self):
        obj = self.obj
        for (data_path, index), keys in self.channels.items():
            for frame, value in sorted(keys.items()):
                if data_path == "location":
                    obj.location[index] = value
                elif data_path == "rotation_euler":
                    obj.rotation_euler[index] = value
                obj.keyframe_insert(data_path, index=index, frame=frame)


def animate(scene, people):
    field = scene["field"]
    grouped = {}
    for sample in scene.get("keyframes") or []:
        grouped.setdefault(sample["performer"], []).append(sample)
    posed = (
        "spine",
        "thigh.L",
        "shin.L",
        "foot.L",
        "thigh.R",
        "shin.R",
        "foot.R",
        "arm.L",
        "forearm.L",
        "arm.R",
        "forearm.R",
    )
    for index, samples in grouped.items():
        parts = people.get(index)
        if parts is None:
            continue
        samples = sorted(samples, key=lambda item: int(item["frame"]))
        unique = []
        seen = set()
        for sample in samples:
            frame = int(sample["frame"])
            if frame in seen:
                continue
            seen.add(frame)
            unique.append(sample)
        if not unique:
            continue
        root = Baker(parts["root"])
        limbs = {name: Baker(parts[name]) for name in posed if name in parts}
        grips = [grip for grip in parts.get("grips") or [] if "bell_dir" in grip]
        grip_bakers = [Baker(grip) for grip in grips]
        flap = parts.get("flap")
        flap_baker = Baker(flap) if flap is not None else None
        prop = parts.get("prop") or "none"
        for cursor, sample in enumerate(unique):
            frame = int(sample["frame"])
            world = to_world(field, sample["x"], sample["y"], 0.0)
            root.key("location", 0, frame, world[0])
            root.key("location", 1, frame, world[1])
            root.key("location", 2, frame, 0.0)
            root.key("rotation_euler", 2, frame, float(sample.get("facing") or 0.0))
            gait = sample_gait(sample)
            step = int(sample.get("step") or 0)
            pose = pose_for(gait, step, 0.0, prop)
            _key_pose(limbs, grip_bakers, grips, pose, gait, frame)
            if cursor + 1 < len(unique):
                nxt = unique[cursor + 1]
                mid = (frame + int(nxt["frame"])) // 2
                if frame < mid < int(nxt["frame"]):
                    mid_gait = gait
                    if gait == "attention":
                        mid_gait = sample_gait(nxt)
                    mid_pose = pose_for(mid_gait, step, 0.5, prop)
                    _key_pose(limbs, grip_bakers, grips, mid_pose, mid_gait, mid)
                    root.key("location", 2, mid, 0.028)
                    if flap_baker is not None:
                        flap_baker.key("rotation_euler", 1, frame, 0.0)
                        flap_baker.key("rotation_euler", 1, mid, 0.18 if step % 2 == 0 else -0.18)
        root.write()
        for baker in limbs.values():
            baker.write()
        for baker in grip_bakers:
            baker.write()
        if flap_baker is not None:
            flap_baker.write()
        if unique:
            first = unique[0]
            pose = pose_for(sample_gait(first), int(first.get("step") or 0), 0.0, prop)
            apply_pose(parts, pose, {"order": sample_gait(first) == "attention"})
            world = to_world(field, first["x"], first["y"], 0.0)
            parts["root"].location = world
            parts["root"].rotation_euler = (0.0, 0.0, float(first.get("facing") or 0.0))


def _key_pose(limbs, grip_bakers, grips, pose, gait, frame):
    for name, baker in limbs.items():
        rx, ry, rz = pose.get(name, (0.0, 0.0, 0.0))
        baker.key("rotation_euler", 0, frame, rx)
        baker.key("rotation_euler", 1, frame, ry)
        baker.key("rotation_euler", 2, frame, rz)
    for baker, grip in zip(grip_bakers, grips):
        side = grip.get("side", "R")
        direction = grip["bell_dir"]
        if gait == "attention" and "order_dir" in grip:
            direction = grip["order_dir"]
        if f"arm.{side}" not in pose:
            continue
        rx, ry, rz = grip_euler(pose, side, direction)
        baker.key("rotation_euler", 0, frame, rx)
        baker.key("rotation_euler", 1, frame, ry)
        baker.key("rotation_euler", 2, frame, rz)


def bone_local_euler(pose_bone, euler):
    basis = pose_bone.bone.matrix.to_4x4()
    world = Euler(euler, "XYZ").to_matrix().to_4x4()
    local = basis.inverted() @ world @ basis
    return local.to_euler("XYZ")


def build_rig(meshes, mats, hide=True):
    """Author MarcherRig and the five march actions. Hidden from the show render."""
    try:
        _build_rig(meshes, mats, hide)
    except Exception as exc:
        print(f"marcher rig skipped: {exc}", flush=True)


def hide_marcher():
    collection = bpy.data.collections.get("Marcher")
    if collection is not None:
        for obj in collection.objects:
            obj.hide_render = True
    for layer in bpy.context.view_layer.layer_collection.children:
        if layer.name == "Marcher":
            layer.exclude = True
            break


def _build_rig(meshes, mats, hide):
    collection = bpy.data.collections.new("Marcher")
    bpy.context.scene.collection.children.link(collection)
    arm = bpy.data.armatures.new("MarcherArmature")
    rig = bpy.data.objects.new("MarcherRig", arm)
    rig.rotation_mode = "XYZ"
    rig.show_in_front = True
    link(rig, collection)
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")

    def add_bone(name, head, tail, parent=None):
        bone = arm.edit_bones.new(name)
        bone.head = head
        bone.tail = tail
        bone.use_connect = False
        if parent is not None:
            bone.parent = arm.edit_bones[parent]
        try:
            bone.align_roll(Vector((0.0, -1.0, 0.0)))
        except ValueError:
            pass
        return bone

    add_bone("spine", (0.0, 0.0, 0.84), (0.0, 0.0, 1.42))
    add_bone("head", (0.0, 0.0, 1.42), (0.0, 0.0, 1.72), "spine")
    for side, sign in (("L", -1.0), ("R", 1.0)):
        add_bone(
            f"arm.{side}",
            (sign * SHOULDER_X, 0.0, SHOULDER_Z),
            (sign * SHOULDER_X, 0.0, SHOULDER_Z - UPPER),
            "spine",
        )
        add_bone(
            f"forearm.{side}",
            (sign * SHOULDER_X, 0.0, SHOULDER_Z - UPPER),
            (sign * SHOULDER_X, 0.0, SHOULDER_Z - UPPER - FORE),
            f"arm.{side}",
        )
        add_bone(f"thigh.{side}", (sign * 0.09, 0.0, HIP_Z), (sign * 0.09, 0.0, KNEE_Z), "spine")
        add_bone(f"shin.{side}", (sign * 0.09, 0.0, KNEE_Z), (sign * 0.09, 0.0, ANKLE_Z), f"thigh.{side}")
        add_bone(f"foot.{side}", (sign * 0.09, 0.0, ANKLE_Z), (sign * 0.09, -0.18, ANKLE_Z - 0.02), f"shin.{side}")
    bpy.ops.object.mode_set(mode="POSE")
    for pose_bone in rig.pose.bones:
        pose_bone.rotation_mode = "XYZ"

    # A trumpet player parented to the bones, so the action drives a real figure.
    asset = spawn(
        "asset",
        (196, 48, 40),
        "trumpet",
        1.75,
        meshes,
        mats,
        collection,
        detailed=True,
    )
    bpy.context.view_layer.update()
    bone_of = {
        "body": "spine",
        "spine": "spine",
        "thigh.L": "thigh.L",
        "shin.L": "shin.L",
        "foot.L": "foot.L",
        "thigh.R": "thigh.R",
        "shin.R": "shin.R",
        "foot.R": "foot.R",
        "arm.L": "arm.L",
        "forearm.L": "forearm.L",
        "arm.R": "arm.R",
        "forearm.R": "forearm.R",
    }
    # Snapshot first. Reparenting the spine changes every child matrix.
    worlds = {}
    for name in bone_of:
        obj = asset.get(name)
        if obj is not None:
            worlds[name] = obj.matrix_world.copy()
    # Reparent posed pieces onto the armature. Props stay on the forearm object.
    for name, bone_name in bone_of.items():
        obj = asset.get(name)
        if obj is None:
            continue
        obj.parent = rig
        obj.parent_type = "BONE"
        obj.parent_bone = bone_name
        obj.matrix_world = worlds[name]

    actions = {
        "march.forward": "forward",
        "march.back": "back",
        "march.mark": "mark",
        "march.attention": "attention",
        "march.horn": "horn",
    }
    if rig.animation_data is None:
        rig.animation_data_create()
    for action_name, gait in actions.items():
        action = bpy.data.actions.new(action_name)
        action.use_fake_user = True
        rig.animation_data.action = action
        # Full stride: left plant, pass, right plant, pass, left plant.
        keys = (
            (1, 0, 0.0),
            (9, 0, 0.5),
            (17, 1, 0.0),
            (25, 1, 0.5),
            (33, 0, 0.0),
        )
        prop = "trumpet" if gait == "horn" else "none"
        for frame, step, phase in keys:
            pose = pose_for(gait, step, phase, prop)
            for bone_name, euler in pose.items():
                pose_bone = rig.pose.bones.get(bone_name)
                if pose_bone is None:
                    continue
                pose_bone.rotation_euler = bone_local_euler(pose_bone, euler)
                pose_bone.keyframe_insert("rotation_euler", frame=frame)
        for curve in getattr(action, "fcurves", []) or []:
            for point in curve.keyframe_points:
                point.interpolation = "LINEAR"
            try:
                modifier = curve.modifiers.new(type="CYCLES")
                modifier.mode_before = "REPEAT"
                modifier.mode_after = "REPEAT"
            except (AttributeError, TypeError):
                pass
    rig.animation_data.action = bpy.data.actions.get("march.forward")
    bpy.ops.object.mode_set(mode="OBJECT")
    bpy.context.scene.frame_set(1)
    if hide:
        hide_marcher()
    print("marcher rig ready", flush=True)


def build_show(scene):
    performers = scene.get("performers") or []
    detailed = len(performers) < 40
    meshes = shared_meshes(detailed)
    mats = Materials()
    build_rig(meshes, mats)
    # The rig build may have left the 3D cursor / mode dirty. Crowd uses fresh objects.
    people = {}
    for person in performers:
        prop = resolve_prop(person.get("prop") or "brass", person.get("index") or 0)
        parts = spawn(
            f"p{person['index']}",
            tuple(person.get("color") or (180, 180, 180)),
            prop,
            person.get("height_m") or 1.75,
            meshes,
            mats,
            detailed=detailed,
        )
        people[person["index"]] = parts
    animate(scene, people)
    kind = "detailed" if detailed else "linked-lod"
    print(f"marchers {len(people)} {kind} prop-rig", flush=True)
    return people


def look_level(obj, target):
    """Aim a camera with zero roll. Local -Z looks at the target, local +Y stays upright."""
    direction = Vector(target) - obj.location
    if direction.length < 1e-6:
        return
    forward = direction.normalized()
    z_axis = -forward
    up = Vector((0.0, 0.0, 1.0))
    x_axis = up.cross(z_axis)
    if x_axis.length < 1e-5:
        x_axis = Vector((1.0, 0.0, 0.0))
    x_axis.normalize()
    y_axis = z_axis.cross(x_axis).normalized()
    rotation = Matrix((x_axis, y_axis, z_axis)).transposed()
    obj.rotation_euler = rotation.to_euler("XYZ")


def studio():
    world = bpy.data.worlds.new("studio")
    bpy.context.scene.world = world
    world.use_nodes = True
    background = world.node_tree.nodes.get("Background")
    if background:
        background.inputs["Color"].default_value = (0.73, 0.74, 0.75, 1.0)
        background.inputs["Strength"].default_value = 0.55
    ground = mesh_obj("studio-ground", new_mesh("ground", lambda bm: add_box(bm, (10, 10, 0.04), (0, 0, -0.02), 0)))
    paint(ground, shade((206, 208, 210), 0.85, 0.0, "cyc"))
    wall = mesh_obj("studio-wall", new_mesh("wall", lambda bm: add_box(bm, (8, 0.06, 4.2), (0, 0, 0), 0)))
    wall.location = (0.0, 2.6, 2.0)
    paint(wall, shade((188, 190, 192), 0.9, 0.0, "cyc-wall"))
    sun_data = bpy.data.lights.new("studio-sun", "SUN")
    sun_data.energy = 3.4
    sun_data.angle = math.radians(8.0)
    sun = bpy.data.objects.new("studio-sun", sun_data)
    link(sun)
    sun.rotation_euler = (math.radians(38), math.radians(-12), math.radians(18))
    fill_data = bpy.data.lights.new("studio-fill", "AREA")
    fill_data.energy = 220
    fill_data.size = 3.0
    fill = bpy.data.objects.new("studio-fill", fill_data)
    fill.location = (-1.4, -2.4, 1.8)
    link(fill)
    look_level(fill, (0.0, 0.0, 1.1))
    rim_data = bpy.data.lights.new("studio-rim", "AREA")
    rim_data.energy = 70
    rim_data.size = 1.5
    rim = bpy.data.objects.new("studio-rim", rim_data)
    rim.location = (0.2, 2.0, 2.2)
    link(rim)
    look_level(rim, (0.0, 0.0, 1.3))
    camera_data = bpy.data.cameras.new("hero-cam")
    camera_data.lens = 42
    camera_data.clip_start = 0.02
    camera_data.clip_end = 40
    camera = bpy.data.objects.new("hero-cam", camera_data)
    # Three-quarter front, level horizon, full body including plume and tuba bell.
    camera.location = (1.25, -4.05, 1.08)
    link(camera)
    look_level(camera, (0.0, 0.02, 0.98))
    bpy.context.scene.camera = camera
    blend = bpy.context.scene
    blend.render.resolution_percentage = 100
    blend.render.film_transparent = False
    try:
        blend.view_settings.view_transform = "AgX"
    except TypeError:
        blend.view_settings.view_transform = "Filmic"
    for engine in ("BLENDER_EEVEE_NEXT", "BLENDER_EEVEE"):
        try:
            blend.render.engine = engine
            break
        except TypeError:
            continue
    eevee = getattr(blend, "eevee", None)
    if eevee is not None and hasattr(eevee, "taa_render_samples"):
        eevee.taa_render_samples = 8
    if eevee is not None and hasattr(eevee, "use_soft_shadows"):
        eevee.use_soft_shadows = True
    return camera


def hero_pose(prop):
    """Stride plus the instrument in playing position, for the close-up."""
    pose = dict(FORWARD_PLANT)
    override = aimed_arms(prop, "horn" if prop not in ALWAYS_AIMED else "forward")
    if prop in ALWAYS_AIMED:
        override = aimed_arms(prop, "forward")
    if override:
        pose.update(override)
    return pose


def clear_prefix(prefix):
    doomed = []
    for obj in bpy.data.objects:
        cursor = obj
        while cursor is not None:
            if cursor.name.startswith(prefix):
                doomed.append(obj)
                break
            cursor = cursor.parent
    for obj in doomed:
        bpy.data.objects.remove(obj, do_unlink=True)


def configure_hero_render(width, height, samples, fps, start, end):
    blend = bpy.context.scene
    blend.render.resolution_x = width
    blend.render.resolution_y = height
    blend.render.fps = fps
    blend.frame_start = start
    blend.frame_end = end
    eevee = getattr(blend, "eevee", None)
    if eevee is not None and hasattr(eevee, "taa_render_samples"):
        eevee.taa_render_samples = samples


def render_still(path, samples):
    blend = bpy.context.scene
    blend.render.image_settings.file_format = "PNG"
    blend.render.filepath = path
    eevee = getattr(blend, "eevee", None)
    if eevee is not None and hasattr(eevee, "taa_render_samples"):
        eevee.taa_render_samples = samples
    bpy.context.view_layer.update()
    bpy.ops.render.render(write_still=True)


def render_heroes(directory):
    os.makedirs(directory, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    studio()
    meshes = shared_meshes(True)
    mats = Materials()
    build_rig(meshes, mats, hide=False)
    asset_path = os.path.join(directory, "marcher.blend")
    bpy.context.scene.frame_set(1)
    bpy.ops.wm.save_as_mainfile(filepath=asset_path)
    print(f"saved {asset_path}", flush=True)
    hide_marcher()
    configure_hero_render(800, 1000, 8, 24, 1, 1)
    for prop, color in HERO_SPECS:
        clear_prefix("fig")
        parts = spawn("fig", color, prop, 1.75, meshes, mats, detailed=True)
        pose = hero_pose(prop)
        apply_pose(parts, pose, {"order": False})
        # Face the audience. The camera sits off to one side for the 3/4 view.
        parts["root"].rotation_euler = (0.0, 0.0, 0.0)
        bpy.context.scene.frame_set(1)
        path = os.path.join(directory, prop)
        print(f"hero still {prop}", flush=True)
        render_still(path, 8)
    # One short turntable of the roll step for every instrument.
    configure_hero_render(640, 800, 4, 12, 1, 24)
    for prop, color in HERO_SPECS:
        clear_prefix("fig")
        parts = spawn("fig", color, prop, 1.75, meshes, mats, detailed=True)
        root = Baker(parts["root"])
        posed_names = (
            "spine",
            "thigh.L",
            "shin.L",
            "foot.L",
            "thigh.R",
            "shin.R",
            "foot.R",
            "arm.L",
            "forearm.L",
            "arm.R",
            "forearm.R",
        )
        limbs = {name: Baker(parts[name]) for name in posed_names}
        grips = [grip for grip in parts.get("grips") or [] if "bell_dir" in grip]
        grip_bakers = [Baker(grip) for grip in grips]
        for frame in range(1, 25):
            yaw = -0.45 + (frame - 1) / 23.0 * math.tau
            root.key("rotation_euler", 2, frame, yaw)
            root.key("location", 0, frame, 0.0)
            root.key("location", 1, frame, 0.0)
            root.key("location", 2, frame, 0.02 if frame % 8 in (3, 4, 5) else 0.0)
            phase = 0.0 if ((frame - 1) // 4) % 2 == 0 else 0.5
            step = 0 if ((frame - 1) // 8) % 2 == 0 else 1
            pose = pose_for("forward", step, phase, prop)
            _key_pose(limbs, grip_bakers, grips, pose, "forward", frame)
        root.write()
        for baker in limbs.values():
            baker.write()
        for baker in grip_bakers:
            baker.write()
        blend = bpy.context.scene
        blend.frame_start = 1
        blend.frame_end = 24
        blend.render.image_settings.file_format = "FFMPEG"
        blend.render.ffmpeg.format = "MPEG4"
        blend.render.ffmpeg.codec = "H264"
        blend.render.ffmpeg.constant_rate_factor = "MEDIUM"
        blend.render.ffmpeg.audio_codec = "AAC"
        outfile = os.path.join(directory, f"{prop}.mp4")
        blend.render.filepath = outfile
        print(f"hero turn {prop}", flush=True)
        bpy.ops.render.render(animation=True)
    print(f"heroes written {directory}", flush=True)
