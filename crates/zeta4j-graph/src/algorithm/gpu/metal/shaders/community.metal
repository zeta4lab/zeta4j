// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

// Community Detection Metal Compute Shader
//
// Apple Silicon GPU에서 커뮤니티 탐지 알고리즘을 실행하기 위한 Metal 셰이더입니다.
// Label Propagation 및 Triangle Counting을 포함합니다.

#include <metal_stdlib>
using namespace metal;

/// Label Propagation 레이블 초기화 커널
kernel void lpa_init_labels(
    device uint* labels [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    labels[node] = node;
}

/// Label Propagation 레이블 업데이트 커널 (동기식)
kernel void lpa_update_labels(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device const uint* labels [[buffer(2)]],
    device uint* new_labels [[buffer(3)]],
    constant uint& num_nodes [[buffer(4)]],
    device atomic_int* changed [[buffer(5)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    // 이웃이 없으면 현재 레이블 유지
    if (start >= end) {
        new_labels[node] = labels[node];
        return;
    }

    // 로컬 레이블 카운트 (작은 해시 테이블)
    const int HASH_SIZE = 32;
    uint local_labels[HASH_SIZE];
    uint local_counts[HASH_SIZE];

    for (int i = 0; i < HASH_SIZE; i++) {
        local_labels[i] = 0xFFFFFFFF;
        local_counts[i] = 0;
    }

    // 이웃들의 레이블 카운트
    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];
        uint neighbor_label = labels[neighbor];

        uint slot = neighbor_label % HASH_SIZE;
        uint original_slot = slot;

        while (true) {
            if (local_labels[slot] == neighbor_label) {
                local_counts[slot]++;
                break;
            } else if (local_labels[slot] == 0xFFFFFFFF) {
                local_labels[slot] = neighbor_label;
                local_counts[slot] = 1;
                break;
            }
            slot = (slot + 1) % HASH_SIZE;
            if (slot == original_slot) break;
        }
    }

    // 가장 빈번한 레이블 찾기
    uint max_count = 0;
    uint best_label = labels[node];

    for (int i = 0; i < HASH_SIZE; i++) {
        if (local_labels[i] != 0xFFFFFFFF) {
            if (local_counts[i] > max_count ||
                (local_counts[i] == max_count && local_labels[i] < best_label)) {
                max_count = local_counts[i];
                best_label = local_labels[i];
            }
        }
    }

    new_labels[node] = best_label;

    if (labels[node] != best_label) {
        atomic_store_explicit(changed, 1, memory_order_relaxed);
    }
}

/// Label Propagation 레이블 복사 커널
kernel void lpa_copy_labels(
    device uint* labels [[buffer(0)]],
    device const uint* new_labels [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    labels[node] = new_labels[node];
}

/// 레이블 존재 표시 커널
kernel void lpa_mark_labels(
    device const uint* labels [[buffer(0)]],
    device int* label_exists [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    uint label = labels[node];
    if (label < num_nodes) {
        label_exists[label] = 1;
    }
}

/// 고유 레이블 카운트 커널
kernel void lpa_count_labels(
    device const int* label_exists [[buffer(0)]],
    device atomic_uint* total_count [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup uint* shared_count [[threadgroup(0)]]
) {
    uint count = 0;
    if (node < num_nodes && label_exists[node] == 1) {
        count = 1;
    }
    shared_count[tid] = count;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_count[tid] += shared_count[tid + s];
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    if (tid == 0) {
        atomic_fetch_add_explicit(total_count, shared_count[0], memory_order_relaxed);
    }
}

/// 가중치 기반 Label Propagation 업데이트 커널
kernel void lpa_update_labels_weighted(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device const float* edge_weights [[buffer(2)]],
    device const uint* labels [[buffer(3)]],
    device uint* new_labels [[buffer(4)]],
    constant uint& num_nodes [[buffer(5)]],
    device atomic_int* changed [[buffer(6)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    if (start >= end) {
        new_labels[node] = labels[node];
        return;
    }

    const int HASH_SIZE = 32;
    uint local_labels[HASH_SIZE];
    float local_weights[HASH_SIZE];

    for (int i = 0; i < HASH_SIZE; i++) {
        local_labels[i] = 0xFFFFFFFF;
        local_weights[i] = 0.0f;
    }

    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];
        uint neighbor_label = labels[neighbor];
        float weight = edge_weights[i];

        uint slot = neighbor_label % HASH_SIZE;
        uint original_slot = slot;

        while (true) {
            if (local_labels[slot] == neighbor_label) {
                local_weights[slot] += weight;
                break;
            } else if (local_labels[slot] == 0xFFFFFFFF) {
                local_labels[slot] = neighbor_label;
                local_weights[slot] = weight;
                break;
            }
            slot = (slot + 1) % HASH_SIZE;
            if (slot == original_slot) break;
        }
    }

    float max_weight = 0.0f;
    uint best_label = labels[node];

    for (int i = 0; i < HASH_SIZE; i++) {
        if (local_labels[i] != 0xFFFFFFFF) {
            if (local_weights[i] > max_weight ||
                (local_weights[i] == max_weight && local_labels[i] < best_label)) {
                max_weight = local_weights[i];
                best_label = local_labels[i];
            }
        }
    }

    new_labels[node] = best_label;

    if (labels[node] != best_label) {
        atomic_store_explicit(changed, 1, memory_order_relaxed);
    }
}

/// Triangle Counting 노드별 커널
kernel void triangle_count_per_node(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device ulong* triangle_counts [[buffer(2)]],
    constant uint& num_nodes [[buffer(3)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    uint start_u = row_offsets[node];
    uint end_u = row_offsets[node + 1];

    ulong count = 0;

    for (uint i = start_u; i < end_u; i++) {
        uint v = col_indices[i];

        if (v <= node) continue;

        uint start_v = row_offsets[v];
        uint end_v = row_offsets[v + 1];

        uint ptr_u = i + 1;
        uint ptr_v = start_v;

        while (ptr_u < end_u && ptr_v < end_v) {
            uint neighbor_u = col_indices[ptr_u];
            uint neighbor_v = col_indices[ptr_v];

            if (neighbor_u <= node) {
                ptr_u++;
                continue;
            }
            if (neighbor_v <= node) {
                ptr_v++;
                continue;
            }

            if (neighbor_u == neighbor_v) {
                count++;
                ptr_u++;
                ptr_v++;
            } else if (neighbor_u < neighbor_v) {
                ptr_u++;
            } else {
                ptr_v++;
            }
        }
    }

    triangle_counts[node] = count;
}

/// Triangle Counting 전체 합산 커널 (32비트 버전)
/// 대부분의 그래프에서 32비트로 충분함
kernel void triangle_count_reduce(
    device const ulong* triangle_counts [[buffer(0)]],
    device atomic_uint* total_count [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup uint* shared_count [[threadgroup(0)]]
) {
    uint count = 0;
    if (node < num_nodes) {
        // ulong to uint truncation (safe for most graphs)
        count = (uint)triangle_counts[node];
    }
    shared_count[tid] = count;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_count[tid] += shared_count[tid + s];
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    if (tid == 0) {
        atomic_fetch_add_explicit(total_count, shared_count[0], memory_order_relaxed);
    }
}

/// Triangle Counting 엣지 기반 커널
kernel void triangle_count_per_edge(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device uint* edge_triangle_counts [[buffer(2)]],
    constant uint& num_edges [[buffer(3)]],
    constant uint& num_nodes [[buffer(4)]],
    uint edge_idx [[thread_position_in_grid]]
) {
    if (edge_idx >= num_edges) return;

    // 이진 검색으로 u 찾기
    uint u = 0;
    uint lo = 0, hi = num_nodes;
    while (lo < hi) {
        uint mid = lo + (hi - lo) / 2;
        if (row_offsets[mid + 1] <= edge_idx) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    u = lo;

    uint v = col_indices[edge_idx];

    if (u >= v) {
        edge_triangle_counts[edge_idx] = 0;
        return;
    }

    uint start_u = row_offsets[u];
    uint end_u = row_offsets[u + 1];
    uint start_v = row_offsets[v];
    uint end_v = row_offsets[v + 1];

    uint count = 0;
    uint ptr_u = start_u;
    uint ptr_v = start_v;

    while (ptr_u < end_u && ptr_v < end_v) {
        uint nu = col_indices[ptr_u];
        uint nv = col_indices[ptr_v];

        if (nu == nv) {
            count++;
            ptr_u++;
            ptr_v++;
        } else if (nu < nv) {
            ptr_u++;
        } else {
            ptr_v++;
        }
    }

    edge_triangle_counts[edge_idx] = count;
}

/// Clustering Coefficient 계산 커널
kernel void compute_clustering_coefficient(
    device const uint* row_offsets [[buffer(0)]],
    device const ulong* triangle_counts [[buffer(1)]],
    device float* clustering_coeffs [[buffer(2)]],
    constant uint& num_nodes [[buffer(3)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    uint degree = row_offsets[node + 1] - row_offsets[node];

    if (degree < 2) {
        clustering_coeffs[node] = 0.0f;
        return;
    }

    ulong triangles = triangle_counts[node];
    ulong possible_triangles = (ulong)degree * (degree - 1) / 2;

    clustering_coeffs[node] = (float)triangles / (float)possible_triangles;
}

/// 변경 플래그 초기화 커널
kernel void clear_changed_flag(
    device atomic_int* changed [[buffer(0)]],
    uint tid [[thread_position_in_grid]]
) {
    if (tid == 0) {
        atomic_store_explicit(changed, 0, memory_order_relaxed);
    }
}

/// 레이블 존재 배열 초기화 커널
kernel void clear_label_exists(
    device int* label_exists [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    label_exists[node] = 0;
}
