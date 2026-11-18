import { Matrix4, PerspectiveCamera } from "three";
import { expect, it } from "vitest";
import type { EdgeInfo, FaceInfo, GroupInfo, InstanceInfo } from "../src/engine";
import { selectMarquee } from "../src/viewport/marquee";

const rect = { left: -0.2, right: 0.2, bottom: -0.2, top: 0.2 };
const ortho = new Matrix4();
type MarqueeScene = Parameters<typeof selectMarquee>[0];
const scene = (geometry: Partial<MarqueeScene>): MarqueeScene => ({
  groups: [], instances: [], faces: [], edgeList: [], edges: [], edgeIds: [],
  positions: [], triFace: [], editPositions: [], editTriFace: [], editing: null, activeGroup: null, tags: {},
  ...geometry,
});
const face = (id: number, group: number | null = null): FaceInfo => ({
  id, group, hidden: false, normal: [0, 0, 1], outer: [], area: 1, material: null,
});
const group = (id: number, parent: number | null = null): GroupInfo => ({
  id, parent, hidden: false, tag: null, name: "Group", faces: 0, locked: false,
});
const instance = (id: number, parent: number | null = null): InstanceInfo => ({
  id, parent, hidden: false, tag: null, name: "Instance", component: 12, transform: ortho.toArray(),
});
const edge = (id: number, a: EdgeInfo["a"], b: EdgeInfo["b"]): EdgeInfo => ({
  id, a, b, soft: false, group: null,
});

it("selects a crossing wire with both endpoints outside the rectangle", () => {
  const segment = edge(5, [-0.9, 0, 0], [0.9, 0, 0]);
  const model = scene({ edges: [...segment.a, ...segment.b], edgeIds: [5], edgeList: [segment] });
  expect(selectMarquee(model, ortho, rect, true).edges).toEqual([5]);
  expect(selectMarquee(model, ortho, rect, false).edges).toEqual([]);
});

it("selects a covering face and misses the opening in a triangulated face", () => {
  const solid = scene({
    faces: [face(1)], positions: [-0.9, -0.9, 0, 0.9, -0.9, 0, 0, 0.9, 0], triFace: [1],
  });
  expect(selectMarquee(solid, ortho, rect, true).faces).toEqual([1]);
  expect(selectMarquee(solid, ortho, rect, false).faces).toEqual([]);

  const positions: number[] = [];
  const strip = (x0: number, y0: number, x1: number, y1: number) => {
    positions.push(x0, y0, 0, x1, y0, 0, x1, y1, 0, x0, y0, 0, x1, y1, 0, x0, y1, 0);
  };
  strip(-0.9, -0.9, -0.3, 0.9);
  strip(0.3, -0.9, 0.9, 0.9);
  strip(-0.3, -0.9, 0.3, -0.3);
  strip(-0.3, 0.3, 0.3, 0.9);
  const opening = scene({ faces: solid.faces, positions, triFace: Array(8).fill(1) });
  expect(selectMarquee(opening, ortho, rect, true).faces).toEqual([]);
});

it("takes the appropriate group or wire-only placement at each editing level", () => {
  const model = scene({
    groups: [group(10), group(11, 10)], instances: [instance(20, 11)],
    edges: [-0.9, 0, 0, 0.9, 0, 0], edgeIds: [20],
  });
  expect(selectMarquee(model, ortho, rect, true)).toEqual({ groups: [10], faces: [], edges: [], instances: [] });
  expect(selectMarquee({ ...model, activeGroup: 10 }, ortho, rect, true).groups).toEqual([11]);
  expect(selectMarquee({ ...model, activeGroup: 11 }, ortho, rect, true).instances).toEqual([20]);
  expect(selectMarquee(model, ortho, rect, false).groups).toEqual([]);
  const wholeView = { left: -1, right: 1, bottom: -1, top: 1 };
  expect(selectMarquee(model, ortho, wholeView, false).groups).toEqual([10]);
});

it("requires all triangle vertices and edges in a window selection", () => {
  const model = scene({
    instances: [instance(1), instance(2)],
    positions: [0, 0, 0, 0.9, -0.9, 0, 0.9, 0.9, 0, 0, 0, 0, 0.1, -0.1, 0, 0.1, 0.1, 0],
    triFace: [1, 2],
    edges: [0, 0, 0, 0.9, 0, 0], edgeIds: [2],
  });
  expect(selectMarquee(model, ortho, rect, true).instances).toEqual([1, 2]);
  expect(selectMarquee(model, ortho, rect, false).instances).toEqual([]);
});

it("uses the component definition's editable geometry inside a placement", () => {
  const model = scene({
    editing: 10, instances: [instance(20)], faces: [face(1)],
    positions: [-0.9, -0.9, 0, 0.9, -0.9, 0, 0, 0.9, 0], triFace: [20],
    editPositions: [-0.9, -0.9, 0, 0.9, -0.9, 0, 0, 0.9, 0], editTriFace: [1],
  });
  expect(selectMarquee(model, ortho, rect, true)).toEqual({ groups: [], faces: [1], edges: [], instances: [] });
});

it("clips perspective segments at the near plane", () => {
  const camera = new PerspectiveCamera(60, 1, 0.1, 10);
  const behind = edge(5, [-0.1, 0, 1], [0.1, 0, 1]);
  const crossing = edge(6, [0, 0, 1], [0, 0, -1]);
  const model = scene({
    edges: [...behind.a, ...behind.b, ...crossing.a, ...crossing.b],
    edgeIds: [5, 6], edgeList: [behind, crossing],
  });
  expect(selectMarquee(model, camera.projectionMatrix, rect, true).edges).toEqual([6]);
  expect(selectMarquee(model, camera.projectionMatrix, rect, false).edges).toEqual([]);
});
