import React from 'react'
import { Label, Table } from 'semantic-ui-react'
import 'style/data-table.scss'

/* global fetch */

class DataTable extends React.Component {
  constructor (props) {
    super(props)

    this.state = {
      isLoading: true,
      headers: [],
      rows: []
    }
  }

  async componentDidMount () {
    this.setState({ isLoading: true })

    let [headers, rows] = await this.getData()

    this.setState({
      isLoading: false,
      headers,
      rows
    })
  }

  async getData () {
    let services = await fetch('api/services')
      .then((res) => res.json())

    if (!services.length) {
      return [null, null]
    }

    return [
      Object.keys(services[0]),
      services.map((row) => Object.values(row))
    ]
  }

  render () {
    return (
      <Table celled>
        <Table.Header>
          <Table.Row>
            {
              this.state.headers.map((el) => {
                return <Table.HeaderCell>{el}</Table.HeaderCell>
              })
            }
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {
            this.state.rows.map((fields) => {
              let cells = []
              for (let field of fields) {
                cells.push(<Table.Cell><Label>{field}</Label></Table.Cell>)
              }
              return <Table.Row>{cells}</Table.Row>
            })
          }
        </Table.Body>
      </Table>
    )
  }
}

export default DataTable
