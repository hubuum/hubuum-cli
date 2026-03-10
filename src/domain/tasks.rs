use hubuum_client::{TaskEventResponse, TaskQueueStateResponse, TaskResponse};

transparent_record!(TaskEventRecord, TaskEventResponse);
transparent_record!(TaskQueueStateRecord, TaskQueueStateResponse);
transparent_record!(TaskRecord, TaskResponse);
