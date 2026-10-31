// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

// Node2Vec Metal Compute Shader
//
// Apple Silicon GPU에서 편향된 랜덤 워크를 수행하기 위한 Metal 셰이더입니다.
// Node2Vec 논문의 p, q 파라미터를 사용한 2차 마르코프 체인 기반 워크입니다.

#include <metal_stdlib>
using namespace metal;

/// LCG 기반 간단한 랜덤 생성기
uint lcg_next(thread uint& state) {
    state = state * 1103515245u + 12345u;
    return (state >> 16) & 0x7fff;
}

/// 0~1 사이 랜덤 float 생성
float random_float(thread uint& state) {
    return (float)lcg_next(state) / 32767.0f;
}

/// Node2Vec 랜덤 워크 커널
kernel void node2vec_random_walk(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device uint* walks [[buffer(2)]],
    device uint* walk_lengths [[buffer(3)]],
    device const uint* start_nodes [[buffer(4)]],
    constant uint& num_walks [[buffer(5)]],
    constant uint& walk_length [[buffer(6)]],
    constant float& p [[buffer(7)]],
    constant float& q [[buffer(8)]],
    constant uint& seed [[buffer(9)]],
    uint walk_id [[thread_position_in_grid]]
) {
    if (walk_id >= num_walks) return;

    // 랜덤 시드 초기화
    uint rng_state = seed ^ (walk_id * 2654435761u);
    lcg_next(rng_state);

    // 워크 시작 위치
    device uint* walk = walks + walk_id * walk_length;

    uint current = start_nodes[walk_id];
    walk[0] = current;

    uint prev = 0xFFFFFFFF;
    uint actual_length = 1;

    for (uint step = 1; step < walk_length; step++) {
        uint start = row_offsets[current];
        uint end = row_offsets[current + 1];
        uint degree = end - start;

        if (degree == 0) break;

        // 이전 노드의 이웃 정보
        uint prev_start = 0, prev_end = 0;
        if (prev != 0xFFFFFFFF) {
            prev_start = row_offsets[prev];
            prev_end = row_offsets[prev + 1];
        }

        // 가중치 합 계산
        float total_weight = 0.0f;
        for (uint i = start; i < end; i++) {
            uint neighbor = col_indices[i];
            float weight;

            if (prev == 0xFFFFFFFF) {
                weight = 1.0f;
            } else if (neighbor == prev) {
                weight = 1.0f / p;
            } else {
                bool connected_to_prev = false;
                for (uint j = prev_start; j < prev_end; j++) {
                    if (col_indices[j] == neighbor) {
                        connected_to_prev = true;
                        break;
                    }
                }
                weight = connected_to_prev ? 1.0f : (1.0f / q);
            }
            total_weight += weight;
        }

        // 가중치 기반 랜덤 선택
        float threshold = random_float(rng_state) * total_weight;
        float cumulative = 0.0f;
        uint next = current;

        for (uint i = start; i < end; i++) {
            uint neighbor = col_indices[i];
            float weight;

            if (prev == 0xFFFFFFFF) {
                weight = 1.0f;
            } else if (neighbor == prev) {
                weight = 1.0f / p;
            } else {
                bool connected_to_prev = false;
                for (uint j = prev_start; j < prev_end; j++) {
                    if (col_indices[j] == neighbor) {
                        connected_to_prev = true;
                        break;
                    }
                }
                weight = connected_to_prev ? 1.0f : (1.0f / q);
            }

            cumulative += weight;
            if (cumulative >= threshold) {
                next = neighbor;
                break;
            }
        }

        walk[step] = next;
        actual_length++;

        prev = current;
        current = next;
    }

    walk_lengths[walk_id] = actual_length;
}

/// DeepWalk 스타일 단순 랜덤 워크 (p=q=1 최적화)
kernel void deepwalk_random_walk(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device uint* walks [[buffer(2)]],
    device uint* walk_lengths [[buffer(3)]],
    device const uint* start_nodes [[buffer(4)]],
    constant uint& num_walks [[buffer(5)]],
    constant uint& walk_length [[buffer(6)]],
    constant uint& seed [[buffer(7)]],
    uint walk_id [[thread_position_in_grid]]
) {
    if (walk_id >= num_walks) return;

    uint rng_state = seed ^ (walk_id * 2654435761u);
    lcg_next(rng_state);

    device uint* walk = walks + walk_id * walk_length;
    uint current = start_nodes[walk_id];
    walk[0] = current;

    uint actual_length = 1;

    for (uint step = 1; step < walk_length; step++) {
        uint start = row_offsets[current];
        uint end = row_offsets[current + 1];
        uint degree = end - start;

        if (degree == 0) break;

        uint idx = lcg_next(rng_state) % degree;
        current = col_indices[start + idx];

        walk[step] = current;
        actual_length++;
    }

    walk_lengths[walk_id] = actual_length;
}

/// 시작 노드 배열 생성
kernel void generate_start_nodes(
    device uint* start_nodes [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    constant uint& walks_per_node [[buffer(2)]],
    uint idx [[thread_position_in_grid]]
) {
    uint total = num_nodes * walks_per_node;
    if (idx >= total) return;

    uint node = idx / walks_per_node;
    start_nodes[idx] = node;
}

/// 노드 등장 빈도 계산
kernel void count_node_frequency(
    device const uint* walks [[buffer(0)]],
    device const uint* walk_lengths [[buffer(1)]],
    device atomic_uint* node_counts [[buffer(2)]],
    constant uint& num_walks [[buffer(3)]],
    constant uint& walk_length [[buffer(4)]],
    uint walk_id [[thread_position_in_grid]]
) {
    if (walk_id >= num_walks) return;

    device const uint* walk = walks + walk_id * walk_length;
    uint length = walk_lengths[walk_id];

    for (uint i = 0; i < length; i++) {
        uint node = walk[i];
        atomic_fetch_add_explicit(&node_counts[node], 1u, memory_order_relaxed);
    }
}

/// 엣지 동시출현(co-occurrence) 계산
kernel void count_edge_cooccurrence(
    device const uint* walks [[buffer(0)]],
    device const uint* walk_lengths [[buffer(1)]],
    device atomic_uint* edge_counts [[buffer(2)]],
    constant uint& num_walks [[buffer(3)]],
    constant uint& walk_length [[buffer(4)]],
    constant uint& window_size [[buffer(5)]],
    constant uint& num_nodes [[buffer(6)]],
    uint walk_id [[thread_position_in_grid]]
) {
    if (walk_id >= num_walks) return;

    device const uint* walk = walks + walk_id * walk_length;
    uint length = walk_lengths[walk_id];

    for (uint i = 0; i < length; i++) {
        uint center = walk[i];

        for (uint j = 1; j <= window_size && i + j < length; j++) {
            uint context = walk[i + j];
            if (center != context) {
                uint n1 = (center < context) ? center : context;
                uint n2 = (center < context) ? context : center;
                atomic_fetch_add_explicit(&edge_counts[n1 * num_nodes + n2], 1u, memory_order_relaxed);
            }
        }
    }
}

/// 워크 길이 합계 계산
kernel void sum_walk_lengths(
    device const uint* walk_lengths [[buffer(0)]],
    device atomic_uint* total_length [[buffer(1)]],
    constant uint& num_walks [[buffer(2)]],
    uint walk_id [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup uint* shared_sum [[threadgroup(0)]]
) {
    uint length = 0;
    if (walk_id < num_walks) {
        length = walk_lengths[walk_id];
    }
    shared_sum[tid] = length;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_sum[tid] += shared_sum[tid + s];
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    if (tid == 0) {
        atomic_fetch_add_explicit(total_length, shared_sum[0], memory_order_relaxed);
    }
}

/// 워크 초기화 (무효값으로)
kernel void init_walks(
    device uint* walks [[buffer(0)]],
    constant uint& total_size [[buffer(1)]],
    uint idx [[thread_position_in_grid]]
) {
    if (idx >= total_size) return;
    walks[idx] = 0xFFFFFFFF;
}

/// 노드 카운트 초기화
kernel void init_node_counts(
    device uint* node_counts [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    uint idx [[thread_position_in_grid]]
) {
    if (idx >= num_nodes) return;
    node_counts[idx] = 0;
}
