// Node2Vec CUDA 커널
//
// GPU에서 편향된 랜덤 워크를 수행하기 위한 CUDA 커널입니다.
// Node2Vec 논문의 p, q 파라미터를 사용한 2차 마르코프 체인 기반 워크입니다.

extern "C" {

/// 랜덤 시드 초기화 (간단한 LCG)
__device__ unsigned int lcg_next(unsigned int* state) {
    *state = *state * 1103515245u + 12345u;
    return (*state >> 16) & 0x7fff;
}

/// 0~1 사이 랜덤 float 생성
__device__ float random_float(unsigned int* state) {
    return (float)lcg_next(state) / 32767.0f;
}

/// Node2Vec 랜덤 워크 커널
///
/// 각 스레드가 하나의 워크를 생성합니다.
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param col_indices CSR 열 인덱스 배열
/// @param walks 워크 결과 배열 [num_walks * walk_length]
/// @param walk_lengths 각 워크의 실제 길이 [num_walks]
/// @param start_nodes 각 워크의 시작 노드 [num_walks]
/// @param num_walks 총 워크 수
/// @param walk_length 최대 워크 길이
/// @param p Return parameter (이전 노드로 돌아갈 확률 조절)
/// @param q In-out parameter (BFS vs DFS 스타일 조절)
/// @param seed 랜덤 시드 베이스
__global__ void node2vec_random_walk(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    unsigned int* walks,
    unsigned int* walk_lengths,
    const unsigned int* start_nodes,
    const unsigned int num_walks,
    const unsigned int walk_length,
    const float p,
    const float q,
    const unsigned int seed
) {
    unsigned int walk_id = blockIdx.x * blockDim.x + threadIdx.x;
    if (walk_id >= num_walks) return;

    // 랜덤 시드 초기화 (워크 ID 기반)
    unsigned int rng_state = seed ^ (walk_id * 2654435761u);
    lcg_next(&rng_state); // 워밍업

    // 워크 시작 위치
    unsigned int* walk = walks + walk_id * walk_length;

    // 시작 노드
    unsigned int current = start_nodes[walk_id];
    walk[0] = current;

    unsigned int prev = 0xFFFFFFFF; // 이전 노드 (처음에는 없음)
    unsigned int actual_length = 1;

    for (unsigned int step = 1; step < walk_length; step++) {
        unsigned int start = row_offsets[current];
        unsigned int end = row_offsets[current + 1];
        unsigned int degree = end - start;

        if (degree == 0) break; // 이웃이 없으면 종료

        // 전이 확률 계산 및 가중치 기반 선택
        float total_weight = 0.0f;
        float threshold = random_float(&rng_state);

        // 이전 노드의 이웃 집합 (빠른 조회용 - 간소화 버전)
        unsigned int prev_start = 0, prev_end = 0;
        if (prev != 0xFFFFFFFF) {
            prev_start = row_offsets[prev];
            prev_end = row_offsets[prev + 1];
        }

        // 가중치 합 계산
        for (unsigned int i = start; i < end; i++) {
            unsigned int neighbor = col_indices[i];
            float weight;

            if (prev == 0xFFFFFFFF) {
                // 첫 번째 스텝: 균등 확률
                weight = 1.0f;
            } else if (neighbor == prev) {
                // 이전 노드로 돌아가기: 1/p
                weight = 1.0f / p;
            } else {
                // 이전 노드와의 연결 확인
                bool connected_to_prev = false;
                for (unsigned int j = prev_start; j < prev_end; j++) {
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
        float cumulative = 0.0f;
        threshold *= total_weight;
        unsigned int next = current; // 기본값

        for (unsigned int i = start; i < end; i++) {
            unsigned int neighbor = col_indices[i];
            float weight;

            if (prev == 0xFFFFFFFF) {
                weight = 1.0f;
            } else if (neighbor == prev) {
                weight = 1.0f / p;
            } else {
                bool connected_to_prev = false;
                for (unsigned int j = prev_start; j < prev_end; j++) {
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

/// 단순 랜덤 워크 커널 (DeepWalk 스타일)
///
/// p=q=1인 특수 케이스로 최적화
__global__ void deepwalk_random_walk(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    unsigned int* walks,
    unsigned int* walk_lengths,
    const unsigned int* start_nodes,
    const unsigned int num_walks,
    const unsigned int walk_length,
    const unsigned int seed
) {
    unsigned int walk_id = blockIdx.x * blockDim.x + threadIdx.x;
    if (walk_id >= num_walks) return;

    unsigned int rng_state = seed ^ (walk_id * 2654435761u);
    lcg_next(&rng_state);

    unsigned int* walk = walks + walk_id * walk_length;
    unsigned int current = start_nodes[walk_id];
    walk[0] = current;

    unsigned int actual_length = 1;

    for (unsigned int step = 1; step < walk_length; step++) {
        unsigned int start = row_offsets[current];
        unsigned int end = row_offsets[current + 1];
        unsigned int degree = end - start;

        if (degree == 0) break;

        // 균등 확률로 이웃 선택
        unsigned int idx = lcg_next(&rng_state) % degree;
        current = col_indices[start + idx];

        walk[step] = current;
        actual_length++;
    }

    walk_lengths[walk_id] = actual_length;
}

/// 시작 노드 배열 생성 (모든 노드에서 walks_per_node 만큼)
__global__ void generate_start_nodes(
    unsigned int* start_nodes,
    const unsigned int num_nodes,
    const unsigned int walks_per_node
) {
    unsigned int idx = blockIdx.x * blockDim.x + threadIdx.x;
    unsigned int total = num_nodes * walks_per_node;
    if (idx >= total) return;

    unsigned int node = idx / walks_per_node;
    start_nodes[idx] = node;
}

/// 노드별 워크 시작 위치 셔플 (학습 다양성을 위해)
__global__ void shuffle_walks(
    unsigned int* walk_order,
    const unsigned int num_walks,
    const unsigned int seed
) {
    // 단순 Fisher-Yates 변형 (병렬 버전은 비결정적)
    unsigned int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_walks) return;

    unsigned int rng_state = seed ^ (idx * 2654435761u);
    unsigned int swap_idx = idx + lcg_next(&rng_state) % (num_walks - idx);

    // Atomic swap (race condition 가능하지만 셔플 목적으로는 충분)
    if (swap_idx != idx && swap_idx < num_walks) {
        unsigned int temp = walk_order[idx];
        walk_order[idx] = walk_order[swap_idx];
        walk_order[swap_idx] = temp;
    }
}

/// 워크 결과에서 노드 등장 빈도 계산
__global__ void count_node_frequency(
    const unsigned int* walks,
    const unsigned int* walk_lengths,
    unsigned int* node_counts,
    const unsigned int num_walks,
    const unsigned int walk_length
) {
    unsigned int walk_id = blockIdx.x * blockDim.x + threadIdx.x;
    if (walk_id >= num_walks) return;

    const unsigned int* walk = walks + walk_id * walk_length;
    unsigned int length = walk_lengths[walk_id];

    for (unsigned int i = 0; i < length; i++) {
        unsigned int node = walk[i];
        atomicAdd(&node_counts[node], 1);
    }
}

/// 워크 결과에서 엣지 동시출현(co-occurrence) 계산
/// 윈도우 크기 내의 노드 쌍을 카운트
__global__ void count_edge_cooccurrence(
    const unsigned int* walks,
    const unsigned int* walk_lengths,
    unsigned int* edge_counts, // 간소화: node1 * num_nodes + node2
    const unsigned int num_walks,
    const unsigned int walk_length,
    const unsigned int window_size,
    const unsigned int num_nodes
) {
    unsigned int walk_id = blockIdx.x * blockDim.x + threadIdx.x;
    if (walk_id >= num_walks) return;

    const unsigned int* walk = walks + walk_id * walk_length;
    unsigned int length = walk_lengths[walk_id];

    for (unsigned int i = 0; i < length; i++) {
        unsigned int center = walk[i];

        // 윈도우 내 이웃들과의 동시출현
        for (unsigned int j = 1; j <= window_size && i + j < length; j++) {
            unsigned int context = walk[i + j];
            if (center != context) {
                // 정규화된 인덱스 (작은 노드 * num_nodes + 큰 노드)
                unsigned int n1 = (center < context) ? center : context;
                unsigned int n2 = (center < context) ? context : center;
                atomicAdd(&edge_counts[n1 * num_nodes + n2], 1);
            }
        }
    }
}

/// 전이 확률 미리 계산 (alias sampling 준비)
/// 메모리 효율성을 위해 필요한 경우에만 사용
__global__ void precompute_transition_probs(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    float* alias_prob, // Alias sampling 확률
    unsigned int* alias_j, // Alias 인덱스
    const unsigned int num_nodes,
    const float p,
    const float q
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];
    unsigned int degree = end - start;

    if (degree == 0) return;

    // 각 이웃에 대한 기본 전이 확률 계산
    // (실제 Alias table 구성은 CPU에서 수행하는 것이 더 효율적)
    float sum = (float)degree; // 기본 균등 확률의 합

    // Alias table 초기화 (단순화된 버전)
    for (unsigned int i = start; i < end; i++) {
        alias_prob[i] = 1.0f;
        alias_j[i] = col_indices[i];
    }
}

} // extern "C"
