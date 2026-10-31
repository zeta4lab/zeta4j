// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

// PageRank Metal Compute Shader
//
// Apple Silicon GPU에서 PageRank 알고리즘을 실행하기 위한 Metal 셰이더입니다.
// CSR (Compressed Sparse Row) 포맷의 그래프를 사용합니다.

#include <metal_stdlib>
using namespace metal;

/// PageRank 파라미터 구조체
struct PageRankParams {
    uint num_nodes;
    float damping;
    float teleport;
    float dangling_contribution;
};

/// PageRank 반복 계산 커널
///
/// 각 노드의 새로운 PageRank 점수를 계산합니다.
/// new_score[i] = teleport + damping * sum(old_score[j] / out_degree[j]) for j in incoming[i]
kernel void pagerank_iteration(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device const uint* out_degrees [[buffer(2)]],
    device const float* old_scores [[buffer(3)]],
    device float* new_scores [[buffer(4)]],
    constant PageRankParams& params [[buffer(5)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= params.num_nodes) return;

    float sum = 0.0f;
    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    // 들어오는 엣지로부터 점수 수집
    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];
        uint degree = out_degrees[neighbor];
        if (degree > 0) {
            sum += old_scores[neighbor] / float(degree);
        }
    }

    new_scores[node] = params.teleport + params.damping * sum;
}

/// PageRank 반복 계산 (댕글링 기여도 포함)
///
/// 댕글링 노드 기여도가 미리 계산된 경우 사용합니다.
kernel void pagerank_iteration_with_dangling(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device const uint* out_degrees [[buffer(2)]],
    device const float* old_scores [[buffer(3)]],
    device float* new_scores [[buffer(4)]],
    constant PageRankParams& params [[buffer(5)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= params.num_nodes) return;

    float sum = 0.0f;
    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    // 들어오는 엣지로부터 점수 수집
    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];
        uint degree = out_degrees[neighbor];
        if (degree > 0) {
            sum += old_scores[neighbor] / float(degree);
        }
    }

    new_scores[node] = params.teleport + params.damping * sum + params.dangling_contribution;
}

/// 댕글링 노드 점수 합계 계산 (Threadgroup 리덕션)
///
/// 나가는 엣지가 없는 노드들의 점수 합계를 계산합니다.
kernel void compute_dangling_sum(
    device const uint* out_degrees [[buffer(0)]],
    device const float* scores [[buffer(1)]],
    device atomic_float* partial_sum [[buffer(2)]],
    constant uint& num_nodes [[buffer(3)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup float* shared_data [[threadgroup(0)]]
) {
    // 각 스레드가 자신의 노드 처리
    float val = 0.0f;
    if (node < num_nodes && out_degrees[node] == 0) {
        val = scores[node];
    }
    shared_data[tid] = val;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Threadgroup 내 리덕션
    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_data[tid] += shared_data[tid + s];
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    // 첫 번째 스레드가 결과를 atomic으로 추가
    if (tid == 0) {
        atomic_fetch_add_explicit(partial_sum, shared_data[0], memory_order_relaxed);
    }
}

/// Float에 대한 atomic max 헬퍼 함수
/// Metal은 float에 대해 atomic_fetch_max를 지원하지 않으므로,
/// uint로 비트 캐스팅하여 compare-exchange 루프로 구현합니다.
inline void atomic_max_float(device atomic_uint* addr, float val) {
    uint new_val = as_type<uint>(val);
    uint old_val = atomic_load_explicit(addr, memory_order_relaxed);

    while (as_type<float>(old_val) < val) {
        if (atomic_compare_exchange_weak_explicit(addr, &old_val, new_val,
                                                   memory_order_relaxed,
                                                   memory_order_relaxed)) {
            break;
        }
    }
}

/// 수렴 검사 (최대 차이 계산)
///
/// 이전 점수와 새 점수의 최대 차이를 계산합니다.
kernel void compute_max_diff(
    device const float* old_scores [[buffer(0)]],
    device const float* new_scores [[buffer(1)]],
    device atomic_uint* max_diff [[buffer(2)]],  // float를 uint로 비트 캐스팅하여 저장
    constant uint& num_nodes [[buffer(3)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup float* shared_data [[threadgroup(0)]]
) {
    // 각 스레드가 자신의 노드 처리
    float diff = 0.0f;
    if (node < num_nodes) {
        diff = abs(new_scores[node] - old_scores[node]);
    }
    shared_data[tid] = diff;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Threadgroup 내 최대값 리덕션
    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_data[tid] = max(shared_data[tid], shared_data[tid + s]);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    // 첫 번째 스레드가 결과를 atomic max로 업데이트
    if (tid == 0) {
        atomic_max_float(max_diff, shared_data[0]);
    }
}

/// 점수 합계 계산 (정규화용)
kernel void compute_sum(
    device const float* scores [[buffer(0)]],
    device atomic_float* total [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup float* shared_data [[threadgroup(0)]]
) {
    float val = 0.0f;
    if (node < num_nodes) {
        val = scores[node];
    }
    shared_data[tid] = val;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Threadgroup 내 리덕션
    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_data[tid] += shared_data[tid + s];
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    if (tid == 0) {
        atomic_fetch_add_explicit(total, shared_data[0], memory_order_relaxed);
    }
}

/// 점수 정규화
///
/// 모든 점수를 총합으로 나누어 정규화합니다.
kernel void normalize_scores(
    device float* scores [[buffer(0)]],
    constant float& total [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    if (total > 0.0f) {
        scores[node] /= total;
    }
}

/// 나가는 차수 계산 (CSR에서)
///
/// 각 노드의 나가는 엣지 수를 계산합니다.
kernel void compute_out_degrees(
    device const uint* row_offsets [[buffer(0)]],
    device uint* out_degrees [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    out_degrees[node] = row_offsets[node + 1] - row_offsets[node];
}

/// 점수 초기화
///
/// 모든 노드의 초기 점수를 1/N으로 설정합니다.
kernel void init_scores(
    device float* scores [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    scores[node] = 1.0f / float(num_nodes);
}

/// 점수 복사
kernel void copy_scores(
    device const float* src [[buffer(0)]],
    device float* dst [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    dst[node] = src[node];
}
