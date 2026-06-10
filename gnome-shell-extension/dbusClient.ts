import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

// XML declaration of D-Bus interface matching com.auraos.GeminiAssistant1
const GeminiInterfaceXml = `
<node>
  <interface name="com.auraos.GeminiAssistant1">
    <method name="Query">
      <arg direction="in"  name="prompt"     type="s"/>
      <arg direction="in"  name="options"    type="a{sv}"/>
      <arg direction="out" name="response"   type="s"/>
      <arg direction="out" name="task_id"    type="s"/>
    </method>
    <method name="DispatchTask">
      <arg direction="in"  name="task_description"  type="s"/>
      <arg direction="in"  name="context"            type="s"/>
      <arg direction="out" name="task_id"            type="s"/>
    </method>
    <method name="CancelTask">
      <arg direction="in"  name="task_id"    type="s"/>
      <arg direction="out" name="success"    type="b"/>
    </method>
    <method name="GetTaskStatus">
      <arg direction="in"  name="task_id"    type="s"/>
      <arg direction="out" name="status"     type="s"/>
      <arg direction="out" name="result"     type="s"/>
    </method>
    <method name="GetConversationHistory">
      <arg direction="out" name="history"    type="s"/>
    </method>
    <method name="ClearConversation">
    </method>
    <signal name="StreamChunk">
      <arg name="task_id"   type="s"/>
      <arg name="chunk"     type="s"/>
      <arg name="is_final"  type="b"/>
    </signal>
    <signal name="TaskCompleted">
      <arg name="task_id"   type="s"/>
      <arg name="result"    type="s"/>
      <arg name="tools_used" type="as"/>
    </signal>
    <signal name="ProactiveInsight">
      <arg name="insight_type"  type="s"/>
      <arg name="content"       type="s"/>
      <arg name="actions"       type="s"/>
    </signal>
  </interface>
</node>
`;

export class GeminiDBusClient {
    private _proxy: any = null;
    private _signalIds: number[] = [];
    private _listeners: Map<string, Function[]> = new Map();

    constructor() {
        // Create the DBusProxy using Gio
        const GeminiProxyWrapper = Gio.DBusProxy.makeProxyWrapper(GeminiInterfaceXml);
        
        try {
            this._proxy = new GeminiProxyWrapper(
                Gio.DBus.session,
                'com.auraos.GeminiAssistant',
                '/com/auraos/GeminiAssistant'
            );

            // Set up signal listeners
            this._proxy.connectSignal('StreamChunk', (proxy: any, sender: string, [taskId, chunk, isFinal]: [string, string, boolean]) => {
                this._emit('StreamChunk', { taskId, chunk, isFinal });
            });

            this._proxy.connectSignal('TaskCompleted', (proxy: any, sender: string, [taskId, result, toolsUsed]: [string, string, string[]]) => {
                this._emit('TaskCompleted', { taskId, result: JSON.parse(result), toolsUsed });
            });

            this._proxy.connectSignal('ProactiveInsight', (proxy: any, sender: string, [insightType, content, actions]: [string, string, string]) => {
                this._emit('ProactiveInsight', { insightType, content, actions: JSON.parse(actions) });
            });
        } catch (e) {
            logError(e, 'Failed to connect to com.auraos.GeminiAssistant D-Bus service');
        }
    }

    on(event: string, callback: Function) {
        if (!this._listeners.has(event)) {
            this._listeners.set(event, []);
        }
        this._listeners.get(event)!.push(callback);
    }

    private _emit(event: string, data: any) {
        if (this._listeners.has(event)) {
            for (const callback of this._listeners.get(event)!) {
                try {
                    callback(data);
                } catch (err) {
                    logError(err, `Error in D-Bus listener for event ${event}`);
                }
            }
        }
    }

    async query(prompt: string): Promise<{ response: string; taskId: string }> {
        return new Promise((resolve, reject) => {
            // Options are empty dictionary as dynamic GLib.Variant
            const options = new GLib.Variant('a{sv}', {});
            this._proxy.QueryRemote(prompt, options, (res: any, err: any) => {
                if (err) {
                    reject(err);
                } else {
                    const [response, taskId] = res;
                    resolve({ response, taskId });
                }
            });
        });
    }

    async dispatchTask(taskDescription: string, contextJson: string = '{}'): Promise<string> {
        return new Promise((resolve, reject) => {
            this._proxy.DispatchTaskRemote(taskDescription, contextJson, (res: any, err: any) => {
                if (err) {
                    reject(err);
                } else {
                    const [taskId] = res;
                    resolve(taskId);
                }
            });
        });
    }

    async getConversationHistory(): Promise<any[]> {
        return new Promise((resolve, reject) => {
            this._proxy.GetConversationHistoryRemote((res: any, err: any) => {
                if (err) {
                    reject(err);
                } else {
                    const [historyJson] = res;
                    try {
                        resolve(JSON.parse(historyJson));
                    } catch (e) {
                        resolve([]);
                    }
                }
            });
        });
    }

    async clearConversation(): Promise<void> {
        return new Promise((resolve, reject) => {
            this._proxy.ClearConversationRemote((res: any, err: any) => {
                if (err) {
                    reject(err);
                } else {
                    resolve();
                }
            });
        });
    }

    destroy() {
        if (this._proxy) {
            this._proxy = null;
        }
    }
}
