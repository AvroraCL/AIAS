#include "xatlas.h"
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

extern "C" int aias_xatlas_generate(
    const float *positions,
    const uint32_t *indices,
    uint32_t vertex_count,
    uint32_t index_count,
    float *output_uvs,
    uint32_t *atlas_width,
    uint32_t *atlas_height)
{
    if (!positions || !indices || !output_uvs || vertex_count == 0 || index_count == 0 || index_count % 3 != 0)
        return -1;
    xatlas::Atlas *atlas = xatlas::Create();
    if (!atlas)
        return -2;
    xatlas::MeshDecl mesh;
    mesh.vertexCount = vertex_count;
    mesh.vertexPositionData = positions;
    mesh.vertexPositionStride = sizeof(float) * 3;
    mesh.indexCount = index_count;
    mesh.indexData = indices;
    mesh.indexFormat = xatlas::IndexFormat::UInt32;
    const auto add_result = xatlas::AddMesh(atlas, mesh);
    if (add_result != xatlas::AddMeshError::Success) {
        xatlas::Destroy(atlas);
        return 10 + static_cast<int>(add_result);
    }
    xatlas::ChartOptions chart;
    xatlas::PackOptions pack;
    pack.resolution = 0;
    pack.maxChartSize = 4096;
    pack.padding = 16;
    pack.conservative = true;
    xatlas::Generate(atlas, chart, nullptr, pack, nullptr, nullptr);
    if (!atlas->meshes || atlas->meshCount != 1 || atlas->width == 0 || atlas->height == 0) {
        xatlas::Destroy(atlas);
        return -3;
    }
    const float missing = std::numeric_limits<float>::quiet_NaN();
    for (uint32_t i = 0; i < index_count * 2; ++i)
        output_uvs[i] = missing;
    const xatlas::Mesh &result = atlas->meshes[0];
    if (!result.indexArray || result.indexCount != index_count) {
        xatlas::Destroy(atlas);
        return -4;
    }
    // xatlas keeps face order. Resolve its split output vertices through the
    // result index buffer so every original triangle corner receives the UV
    // for the correct chart side of a seam.
    for (uint32_t corner = 0; corner < result.indexCount; ++corner) {
        const uint32_t output_index = result.indexArray[corner];
        if (output_index >= result.vertexCount)
            continue;
        const xatlas::Vertex &vertex = result.vertexArray[output_index];
        if (vertex.atlasIndex < 0)
            continue;
        output_uvs[corner * 2] = vertex.uv[0] / static_cast<float>(atlas->width);
        output_uvs[corner * 2 + 1] = 1.0f - vertex.uv[1] / static_cast<float>(atlas->height);
    }
    if (atlas_width) *atlas_width = atlas->width;
    if (atlas_height) *atlas_height = atlas->height;
    xatlas::Destroy(atlas);
    for (uint32_t i = 0; i < index_count * 2; ++i) {
        if (!std::isfinite(output_uvs[i]))
            return -4;
    }
    return 0;
}
