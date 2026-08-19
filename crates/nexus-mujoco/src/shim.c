// The only code that knows MuJoCo struct layouts. It reads them from the
// real headers at build time and exposes a flat, version-stable extraction
// API; Rust sees opaque pointers and float/int arrays, never a MuJoCo type.
//
// Deliberately no mjr_* (OpenGL) anywhere: mjvScene is the renderer-agnostic
// product — mjr_render itself takes no mjModel — and the studio's own
// renderer draws it.

#include <mujoco/mujoco.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  mjModel* m;
  mjData* d;
  mjvScene scn;
  mjvOption opt;
  mjvCamera cam;
} NxCtx;

NxCtx* nx_load(const char* path, char* err, int errsz) {
  mjModel* m = mj_loadXML(path, NULL, err, errsz);
  if (!m) return NULL;
  NxCtx* c = (NxCtx*)calloc(1, sizeof(NxCtx));
  c->m = m;
  c->d = mj_makeData(m);
  mjv_defaultOption(&c->opt);
  mjv_defaultFreeCamera(m, &c->cam);
  mjv_defaultScene(&c->scn);
  mjv_makeScene(m, &c->scn, 4000);
  mj_forward(m, c->d);
  return c;
}

void nx_free(NxCtx* c) {
  if (!c) return;
  mjv_freeScene(&c->scn);
  mj_deleteData(c->d);
  mj_deleteModel(c->m);
  free(c);
}

int nx_nq(NxCtx* c) { return c->m->nq; }
int nx_nkey(NxCtx* c) { return c->m->nkey; }

// Pose from a keyframe (e.g. the playground scenes' "home"), then forward.
int nx_reset_keyframe(NxCtx* c, int k) {
  if (k < 0 || k >= c->m->nkey) return 0;
  mj_resetDataKeyframe(c->m, c->d, k);
  mj_forward(c->m, c->d);
  return 1;
}

int nx_set_qpos(NxCtx* c, const double* q, int n) {
  if (n > c->m->nq) n = c->m->nq;
  memcpy(c->d->qpos, q, (size_t)n * sizeof(double));
  mj_forward(c->m, c->d);
  return n;
}

// Abstract scene refresh; returns the geom count. Decor (frames, contact
// arrows) is excluded — this is the model, not the debug overlay.
int nx_update(NxCtx* c) {
  mjv_updateScene(c->m, c->d, &c->opt, NULL, &c->cam, mjCAT_STATIC | mjCAT_DYNAMIC, &c->scn);
  return c->scn.ngeom;
}

// f = [pos3, mat9 (row-major), size3, rgba4] — 19 floats.
int nx_geom(NxCtx* c, int i, float* f, int* type, int* dataid, int* category) {
  if (i < 0 || i >= c->scn.ngeom) return 0;
  const mjvGeom* g = c->scn.geoms + i;
  memcpy(f, g->pos, 3 * sizeof(float));
  memcpy(f + 3, g->mat, 9 * sizeof(float));
  memcpy(f + 12, g->size, 3 * sizeof(float));
  memcpy(f + 15, g->rgba, 4 * sizeof(float));
  *type = g->type;
  *dataid = g->dataid;
  *category = g->category;
  return 1;
}

int nx_nmesh(NxCtx* c) { return c->m->nmesh; }

int nx_mesh_counts(NxCtx* c, int id, int* nvert, int* nface, int* nnormal) {
  if (id < 0 || id >= c->m->nmesh) return 0;
  *nvert = c->m->mesh_vertnum[id];
  *nface = c->m->mesh_facenum[id];
  *nnormal = c->m->mesh_normalnum[id];
  return 1;
}

// Positions and normals live in separate index spaces (mesh_face vs
// mesh_facenormal). The face arrays hold MESH-LOCAL indices already — only
// the vertex/normal/face ARRAYS take the adr offset. Subtracting vertadr
// from the indices too shredded every mesh except mesh 0 into slivers.
int nx_mesh_arrays(NxCtx* c, int id, float* vert, int* face, float* normal, int* facenormal) {
  if (id < 0 || id >= c->m->nmesh) return 0;
  const mjModel* m = c->m;
  int va = m->mesh_vertadr[id], vn = m->mesh_vertnum[id];
  int fa = m->mesh_faceadr[id], fn = m->mesh_facenum[id];
  int na = m->mesh_normaladr[id], nn = m->mesh_normalnum[id];
  memcpy(vert, m->mesh_vert + 3 * (size_t)va, 3 * (size_t)vn * sizeof(float));
  memcpy(normal, m->mesh_normal + 3 * (size_t)na, 3 * (size_t)nn * sizeof(float));
  memcpy(face, m->mesh_face + 3 * (size_t)fa, 3 * (size_t)fn * sizeof(int));
  memcpy(facenormal, m->mesh_facenormal + 3 * (size_t)fa, 3 * (size_t)fn * sizeof(int));
  return 1;
}

int nx_nhfield(NxCtx* c) { return c->m->nhfield; }

// size4 = (radius_x, radius_y, z_top, z_bottom); data is nrow*ncol in [0,1].
int nx_hfield_counts(NxCtx* c, int id, int* nrow, int* ncol, double* size4) {
  if (id < 0 || id >= c->m->nhfield) return 0;
  *nrow = c->m->hfield_nrow[id];
  *ncol = c->m->hfield_ncol[id];
  for (int i = 0; i < 4; i++) size4[i] = (double)c->m->hfield_size[4 * (size_t)id + i];
  return 1;
}

int nx_hfield_data(NxCtx* c, int id, float* data) {
  if (id < 0 || id >= c->m->nhfield) return 0;
  int n = c->m->hfield_nrow[id] * c->m->hfield_ncol[id];
  memcpy(data, c->m->hfield_data + c->m->hfield_adr[id], (size_t)n * sizeof(float));
  return 1;
}
