import { Vector4, type Matrix4 } from "three";
import type { Scene } from "../engine";
import type { Sel } from "../selection";

/** The selection rectangle in normalized device coordinates. */
export interface MarqueeRect {
  left: number;
  right: number;
  bottom: number;
  top: number;
}

type Plane = (point: Vector4) => number;
type Target = { kind: keyof Sel; id: number };
type MarqueeScene = Pick<Scene,
  "groups" | "instances" | "faces" | "edgeList" | "edges" | "edgeIds" |
  "positions" | "triFace" | "editPositions" | "editTriFace" | "editing" | "activeGroup" | "tags"
>;

function clipCrosses(points: Vector4[], planes: Plane[]): boolean {
  // Clip against the near/far planes as well as the four sides of the
  // marquee, before dividing by w. Points behind the eye cannot wrap across it.
  if (points.length === 2) {
    let enter = 0;
    let exit = 1;
    for (const plane of planes) {
      const a = plane(points[0]), b = plane(points[1]);
      if (a < 0 && b < 0) return false;
      if (a < 0) enter = Math.max(enter, a / (a - b));
      if (b < 0) exit = Math.min(exit, a / (a - b));
      if (enter > exit) return false;
    }
    return true;
  }
  for (const plane of planes) {
    const clipped: Vector4[] = [];
    let a = points[points.length - 1];
    let da = plane(a);
    for (const b of points) {
      const db = plane(b);
      if ((da < 0) !== (db < 0)) clipped.push(a.clone().lerp(b, da / (da - db)));
      if (db >= 0) clipped.push(b);
      a = b;
      da = db;
    }
    if (!clipped.length) return false;
    points = clipped;
  }
  return true;
}

/** Test the marquee against the scene's tessellated faces and world edges. */
export function selectMarquee(scene: MarqueeScene, viewProjection: Matrix4, rect: MarqueeRect, crossing: boolean): Sel {
  const planes: Plane[] = [
    ({ x, w }) => x - rect.left * w,
    ({ x, w }) => rect.right * w - x,
    ({ y, w }) => y - rect.bottom * w,
    ({ y, w }) => rect.top * w - y,
    ({ z, w }) => z + w,
    ({ z, w }) => w - z,
  ];
  const hits: Record<keyof Sel, Map<number, boolean>> = {
    faces: new Map(), edges: new Map(), groups: new Map(), instances: new Map(),
  };
  const primitive = (target: Target | undefined, xyz: readonly number[], at: number, count: number) => {
    if (!target) return;
    const hit = hits[target.kind];
    if (hit.get(target.id) === crossing) return;
    const points = Array.from({ length: count }, (_, i) => {
      const offset = at + i * 3;
      return new Vector4(xyz[offset], xyz[offset + 1], xyz[offset + 2], 1).applyMatrix4(viewProjection);
    });
    const inside = (point: Vector4) => planes.every((plane) => plane(point) >= 0);
    hit.set(target.id, crossing ? points.some(inside) || clipCrosses(points, planes) : points.every(inside));
  };
  const editing = scene.editing != null;
  const groups = new Map(scene.groups.map((group) => [group.id, group]));
  const groupTargets = new Map<number, Target | undefined>();
  const targetFor = (kind: keyof Sel, id: number, parent: number | null): Target | undefined => {
    if (editing) return parent == null ? { kind, id } : { kind: "groups", id: parent };
    if (parent === scene.activeGroup) return { kind, id };
    if (parent == null) return undefined;
    if (!groupTargets.has(parent)) {
      let child = parent;
      let group = groups.get(child);
      let target: Target | undefined;
      for (let depth = 0; group && depth < groups.size; depth++) {
        if (group.hidden || (group.tag && scene.tags[group.tag] === false)) break;
        if (group.parent === scene.activeGroup) {
          target = { kind: "groups", id: child };
          break;
        }
        if (group.parent == null) break;
        child = group.parent;
        group = groups.get(child);
      }
      groupTargets.set(parent, target);
    }
    return groupTargets.get(parent);
  };
  const faces = new Map(scene.faces.filter((face) => !face.hidden).map((face) => [face.id, targetFor("faces", face.id, face.group)]));
  const edges = new Map(scene.edgeList.filter((edge) => !edge.soft).map((edge) => [edge.id, targetFor("edges", edge.id, edge.group)]));
  const instances = new Map(editing ? [] : scene.instances.map((instance) => [instance.id, targetFor("instances", instance.id, instance.parent)]));
  const positions = editing ? scene.editPositions : scene.positions;
  const triangleOwners = editing ? scene.editTriFace : scene.triFace;
  triangleOwners.forEach((id, i) => primitive(faces.get(id) ?? instances.get(id), positions, i * 9, 3));
  if (editing) {
    for (const edge of scene.edgeList) primitive(edges.get(edge.id), [...edge.a, ...edge.b], 0, 2);
  } else {
    scene.edgeIds.forEach((id, i) => primitive(edges.get(id) ?? instances.get(id), scene.edges, i * 6, 2));
  }
  const ids = (map: Map<number, boolean>) => [...map].filter(([, hit]) => hit).map(([id]) => id);
  return { faces: ids(hits.faces), edges: ids(hits.edges), groups: ids(hits.groups), instances: ids(hits.instances) };
}
