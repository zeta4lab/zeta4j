// BFS (Breadth-First Search) Metal Compute Shader
//
// Apple Silicon GPU에서 레벨 동기화 BFS를 실행하기 위한 Metal 셰이더입니다.
// CSR (Compressed Sparse Row) 포맷의 그래프를 사용합니다.

#include <metal_stdlib>
using namespace metal;

/// BFS 레벨 확장 커널
///
/// 현재 프론티어의 모든 노드에서 이웃을 탐색하여 다음 프론티어를 생성합니다.
kernel void bfs_expand_frontier(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device atomic_int* distances [[buffer(2)]],
    device const int* frontier [[buffer(3)]],
    device atomic_int* next_frontier [[buffer(4)]],
    constant uint& num_nodes [[buffer(5)]],
    constant int& current_level [[buffer(6)]],
    device atomic_int* changed [[buffer(7)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    // 현재 프론티어에 있는 노드만 처리
    if (frontier[node] != 1) return;

    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    // 모든 이웃 탐색
    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];

        // 미방문 노드인 경우 (atomic compare-exchange)
        int expected = -1;
        bool success = atomic_compare_exchange_weak_explicit(
            &distances[neighbor],
            &expected,
            current_level + 1,
            memory_order_relaxed,
            memory_order_relaxed
        );

        if (success) {
            atomic_store_explicit(&next_frontier[neighbor], 1, memory_order_relaxed);
            atomic_store_explicit(changed, 1, memory_order_relaxed);
        }
    }
}

/// BFS 프론티어 초기화 커널
kernel void bfs_clear_frontier(
    device int* frontier [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    frontier[node] = 0;
}

/// 거리 배열 초기화 커널
kernel void bfs_init_distances(
    device int* distances [[buffer(0)]],
    constant uint& num_nodes [[buffer(1)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    distances[node] = -1;
}

/// 시작 노드 설정 커널
kernel void bfs_set_source(
    device int* distances [[buffer(0)]],
    device int* frontier [[buffer(1)]],
    constant uint& source [[buffer(2)]],
    uint tid [[thread_position_in_grid]]
) {
    if (tid == 0) {
        distances[source] = 0;
        frontier[source] = 1;
    }
}

/// Bottom-Up BFS 커널 (대규모 프론티어용)
kernel void bfs_bottom_up(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device atomic_int* distances [[buffer(2)]],
    device const int* frontier [[buffer(3)]],
    device atomic_int* next_frontier [[buffer(4)]],
    constant uint& num_nodes [[buffer(5)]],
    constant int& current_level [[buffer(6)]],
    device atomic_int* changed [[buffer(7)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    // 이미 방문한 노드는 스킵
    int dist = atomic_load_explicit(&distances[node], memory_order_relaxed);
    if (dist != -1) return;

    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    // 프론티어에 있는 이웃이 있는지 확인
    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];

        if (frontier[neighbor] == 1) {
            int expected = -1;
            bool success = atomic_compare_exchange_weak_explicit(
                &distances[node],
                &expected,
                current_level + 1,
                memory_order_relaxed,
                memory_order_relaxed
            );

            if (success) {
                atomic_store_explicit(&next_frontier[node], 1, memory_order_relaxed);
                atomic_store_explicit(changed, 1, memory_order_relaxed);
            }
            return;
        }
    }
}

/// 프론티어 크기 계산 커널
kernel void bfs_count_frontier(
    device const int* frontier [[buffer(0)]],
    device atomic_uint* total_count [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup uint* shared_count [[threadgroup(0)]]
) {
    uint count = 0;
    if (node < num_nodes && frontier[node] == 1) {
        count = 1;
    }
    shared_count[tid] = count;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Threadgroup 내 리덕션
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

/// 경로 역추적을 위한 부모 기록 커널
kernel void bfs_expand_with_parents(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device atomic_int* distances [[buffer(2)]],
    device atomic_int* parents [[buffer(3)]],
    device const int* frontier [[buffer(4)]],
    device atomic_int* next_frontier [[buffer(5)]],
    constant uint& num_nodes [[buffer(6)]],
    constant int& current_level [[buffer(7)]],
    device atomic_int* changed [[buffer(8)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    if (frontier[node] != 1) return;

    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];

        int expected = -1;
        bool success = atomic_compare_exchange_weak_explicit(
            &distances[neighbor],
            &expected,
            current_level + 1,
            memory_order_relaxed,
            memory_order_relaxed
        );

        if (success) {
            atomic_store_explicit(&parents[neighbor], int(node), memory_order_relaxed);
            atomic_store_explicit(&next_frontier[neighbor], 1, memory_order_relaxed);
            atomic_store_explicit(changed, 1, memory_order_relaxed);
        }
    }
}

/// SSSP 거리 초기화 커널
kernel void sssp_init_distances(
    device float* distances [[buffer(0)]],
    constant uint& source [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    distances[node] = (node == source) ? 0.0f : INFINITY;
}

/// SSSP Bellman-Ford 스타일 엣지 릴렉세이션 커널
kernel void sssp_relax_edges(
    device const uint* row_offsets [[buffer(0)]],
    device const uint* col_indices [[buffer(1)]],
    device const float* edge_weights [[buffer(2)]],
    device float* distances [[buffer(3)]],
    constant uint& num_nodes [[buffer(4)]],
    device atomic_int* changed [[buffer(5)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;

    float node_dist = distances[node];
    if (node_dist == INFINITY) return;

    uint start = row_offsets[node];
    uint end = row_offsets[node + 1];

    for (uint i = start; i < end; i++) {
        uint neighbor = col_indices[i];
        float weight = edge_weights[i];
        float new_dist = node_dist + weight;

        // Non-atomic relaxation (may need multiple iterations)
        if (new_dist < distances[neighbor]) {
            distances[neighbor] = new_dist;
            atomic_store_explicit(changed, 1, memory_order_relaxed);
        }
    }
}

/// 다중 소스 BFS 초기화
kernel void bfs_init_multi_source(
    device int* distances [[buffer(0)]],
    device int* frontier [[buffer(1)]],
    device const uint* sources [[buffer(2)]],
    constant uint& num_sources [[buffer(3)]],
    uint idx [[thread_position_in_grid]]
) {
    if (idx >= num_sources) return;

    uint source = sources[idx];
    distances[source] = 0;
    frontier[source] = 1;
}

/// 프론티어 스왑 (next_frontier -> frontier)
kernel void bfs_swap_frontiers(
    device int* frontier [[buffer(0)]],
    device const int* next_frontier [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]]
) {
    if (node >= num_nodes) return;
    frontier[node] = next_frontier[node];
}

/// 연결된 노드 수 계산 (거리가 -1이 아닌 노드 수)
kernel void bfs_count_visited(
    device const int* distances [[buffer(0)]],
    device atomic_uint* count [[buffer(1)]],
    constant uint& num_nodes [[buffer(2)]],
    uint node [[thread_position_in_grid]],
    uint tid [[thread_index_in_threadgroup]],
    uint tg_size [[threads_per_threadgroup]],
    threadgroup uint* shared_count [[threadgroup(0)]]
) {
    uint c = 0;
    if (node < num_nodes && distances[node] != -1) {
        c = 1;
    }
    shared_count[tid] = c;
    threadgroup_barrier(mem_flags::mem_threadgroup);

    for (uint s = tg_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_count[tid] += shared_count[tid + s];
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }

    if (tid == 0) {
        atomic_fetch_add_explicit(count, shared_count[0], memory_order_relaxed);
    }
}
